use anyhow::{Context, Result};
use tokio::process::{Child, ChildStdout, Command};
use tracing::info;

/// Configuração do pipeline de captura de tela.
#[derive(Debug, Clone)]
pub struct CaptureConfig {
    /// Dispositivo DRM/KMS (ex: "/dev/dri/card0")
    pub drm_device: String,
    /// Frames por segundo
    pub framerate: u32,
    /// Bitrate alvo (ex: "4M")
    pub bitrate: String,
    /// Número de frames entre keyframes (GOP)
    pub gop_size: u32,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            drm_device: std::env::var("DRM_DEVICE")
                .unwrap_or_else(|_| "/dev/dri/card0".to_string()),
            framerate: 30,
            bitrate: "4M".to_string(),
            gop_size: 60, // keyframe a cada 2 segundos @ 30fps
        }
    }
}

/// Inicia o processo FFmpeg para captura de tela via kmsgrab.
///
/// ## Estratégia de captura (kmsgrab):
/// - Captura diretamente do framebuffer DRM/KMS — sem portal Wayland
/// - Requer acesso root ou `CAP_SYS_ADMIN`
/// - Funciona em qualquer compositor Wayland (Cosmic, GNOME, KDE, etc.)
///
/// ## Pipeline FFmpeg:
/// `kmsgrab → hwdownload (CPU) → yuv420p → libx264 → H.264 Annex-B → pipe:1`
///
/// Retorna `(Child, ChildStdout)` — o caller deve manter `Child` vivo para o processo continuar.
pub fn spawn_ffmpeg(config: &CaptureConfig) -> Result<(Child, ChildStdout)> {
    let gop_str = config.gop_size.to_string();
    let framerate_str = config.framerate.to_string();

    info!(
        "🎬 Iniciando FFmpeg: kmsgrab device={} fps={} bitrate={}",
        config.drm_device, config.framerate, config.bitrate
    );

    let mut child = Command::new("ffmpeg")
        .args([
            // ── Input: DRM/KMS via kmsgrab ──────────────────────────────
            "-f", "kmsgrab",
            "-device", &config.drm_device,
            "-framerate", &framerate_str,
            "-i", "-",  // lê do DRM/KMS (não de um arquivo)
            // ── Filtros de vídeo ─────────────────────────────────────────
            // hwdownload: transfere frame da GPU/KMS para memória do sistema
            // format=bgr0: converte para formato CPU-friendly
            "-vf", "hwdownload,format=bgr0",
            // ── Codec H.264 ──────────────────────────────────────────────
            "-pix_fmt", "yuv420p",       // formato suportado por todos os browsers
            "-c:v", "libx264",
            "-preset", "ultrafast",      // menor latência de encoding
            "-tune", "zerolatency",      // flush imediato por frame
            // ── Bitrate ──────────────────────────────────────────────────
            "-b:v", &config.bitrate,
            "-maxrate", &config.bitrate,
            "-bufsize", "2M",
            // ── GOP (keyframe interval) ───────────────────────────────────
            "-g", &gop_str,              // keyframe a cada N frames
            "-keyint_min", &gop_str,
            // ── Output: H.264 Annex-B para stdout ────────────────────────
            "-f", "h264",
            "pipe:1",
            // Desabilita áudio e legendas
            "-an",
            "-sn",
        ])
        .stdout(std::process::Stdio::piped())
        // Redireciona stderr para o console do terminal para podermos debugar o erro do FFmpeg.
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true)  // mata FFmpeg quando o Child é dropado
        .spawn()
        .context("Falha ao iniciar ffmpeg. Verifique se ffmpeg está instalado e se tem acesso ao DRM device.")?;

    let stdout = child
        .stdout
        .take()
        .context("FFmpeg não retornou stdout")?;

    Ok((child, stdout))
}
