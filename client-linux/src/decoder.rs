use anyhow::{Context, Result};
use common::DecodedFrame;
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use tracing::{error, info, warn};

#[allow(dead_code)]
pub struct YuvFrame {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

pub struct VideoDecoder {
    child: Option<Child>,
    latest_frame: Arc<Mutex<Option<YuvFrame>>>,
    running: Arc<AtomicBool>,
}

impl VideoDecoder {
    pub fn start(
        codec: u8,
        width: usize,
        height: usize,
        mut frame_rx: tokio::sync::mpsc::Receiver<DecodedFrame>,
    ) -> Result<Self> {
        let codec_name = if codec == 1 { "hevc" } else { "h264" };
        let frame_size = (width * height * 3) / 2;

        info!(
            "🎬 Inicializando pipeline de decodificação FFmpeg (Codec: {}, Resolução: {}x{})...",
            codec_name, width, height
        );

        // Dispara o processo FFmpeg otimizado para baixa latência em pipe
        let mut cmd = Command::new("ffmpeg");
        cmd.args(&[
            "-loglevel", "error",
            "-flags", "low_delay",
            "-fflags", "nobuffer",
            "-f", codec_name,
            "-i", "pipe:0",
            "-f", "rawvideo",
            "-pix_fmt", "yuv420p",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

        // Injeta caminhos de drivers freeworld/VAAPI caso existam
        if std::env::var("LIBVA_DRIVERS_PATH").is_err() {
            for path in &["/usr/local/lib64/dri", "/usr/lib64/dri-freeworld"] {
                if std::path::Path::new(path).exists() {
                    cmd.env("LIBVA_DRIVERS_PATH", path);
                    break;
                }
            }
        }

        let mut child = cmd.spawn().context("Falha ao iniciar processo ffmpeg para decodificação")?;

        let mut stdin = child.stdin.take().context("Falha ao abrir stdin do ffmpeg")?;
        let mut stdout = child.stdout.take().context("Falha ao abrir stdout do ffmpeg")?;

        let running = Arc::new(AtomicBool::new(true));
        let running_write = running.clone();
        let running_read = running.clone();

        let latest_frame = Arc::new(Mutex::new(None));
        let latest_frame_clone = latest_frame.clone();

        // 1. Thread de escrita: NAL frames -> ffmpeg stdin
        tokio::spawn(async move {
            while let Some(decoded_frame) = frame_rx.recv().await {
                if !running_write.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(e) = stdin.write_all(&decoded_frame.data) {
                    warn!("⚠️ Erro ao escrever frame no stdin do decodificador: {}", e);
                    break;
                }
                let _ = stdin.flush();
            }
            info!("🛑 Encerrada thread de escrita do decodificador.");
        });

        // 2. Thread de leitura OS: ffmpeg stdout -> YuvFrame
        thread::spawn(move || {
            let mut buffer = vec![0u8; frame_size];

            while running_read.load(Ordering::Relaxed) {
                match stdout.read_exact(&mut buffer) {
                    Ok(()) => {
                        let frame = YuvFrame {
                            width,
                            height,
                            data: buffer.clone(),
                        };
                        let mut lock = latest_frame_clone.lock().unwrap();
                        *lock = Some(frame);
                    }
                    Err(e) => {
                        if running_read.load(Ordering::Relaxed) {
                            error!("❌ Erro na leitura de frames decodificados do stdout: {}", e);
                        }
                        break;
                    }
                }
            }
            info!("🛑 Encerrada thread de leitura do decodificador.");
        });

        Ok(Self {
            child: Some(child),
            latest_frame,
            running,
        })
    }

    /// Retorna o último frame YUV decodificado, se houver um novo disponível
    pub fn take_latest_frame(&self) -> Option<YuvFrame> {
        let mut lock = self.latest_frame.lock().unwrap();
        lock.take()
    }
}

impl Drop for VideoDecoder {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
