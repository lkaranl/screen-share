use anyhow::{Context, Result};
use tokio::process::{Child, ChildStdout, Command};
use tracing::info;

/// Codecs de vídeo suportados pelo servidor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VideoCodec {
    H264,
    HEVC,
    AV1,
}

/// Resoluções de vídeo suportadas pelo servidor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VideoResolution {
    FHD, // 1080p (1920x1080)
    QHD, // 2K (2560x1440)
    UHD, // 4K (3840x2160)
}

impl VideoResolution {
    pub fn dimensions(&self) -> (u32, u32) {
        match self {
            VideoResolution::FHD => (1920, 1080),
            VideoResolution::QHD => (2560, 1440),
            VideoResolution::UHD => (3840, 2160),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            VideoResolution::FHD => "1080p (FHD)",
            VideoResolution::QHD => "2K (1440p)",
            VideoResolution::UHD => "4K (2160p)",
        }
    }
}

/// Configuração do pipeline de captura de tela.
#[derive(Debug, Clone)]
pub struct CaptureConfig {
    /// Dispositivo DRM/KMS (ex: "/dev/dri/card1")
    pub drm_device: String,
    /// Dispositivo de render VAAPI (ex: "/dev/dri/renderD128")
    pub render_device: String,
    /// Frames por segundo
    pub framerate: u32,
    /// Bitrate alvo (ex: "8M")
    pub bitrate: String,
    /// Número de frames entre keyframes (GOP)
    pub gop_size: u32,
    /// Codec de vídeo a ser utilizado
    pub codec: VideoCodec,
    /// Resolução de vídeo alvo
    pub resolution: VideoResolution,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        let drm_device = std::env::var("DRM_DEVICE").unwrap_or_else(|_| {
            if std::path::Path::new("/dev/dri/card0").exists() && !std::path::Path::new("/dev/dri/card1").exists() {
                "/dev/dri/card0".to_string()
            } else if std::path::Path::new("/dev/dri/card1").exists() {
                "/dev/dri/card1".to_string()
            } else {
                "/dev/dri/card0".to_string()
            }
        });

        let render_device = std::env::var("RENDER_DEVICE").unwrap_or_else(|_| {
            if std::path::Path::new("/dev/dri/renderD128").exists() {
                "/dev/dri/renderD128".to_string()
            } else if std::path::Path::new("/dev/dri/renderD129").exists() {
                "/dev/dri/renderD129".to_string()
            } else {
                "/dev/dri/renderD128".to_string()
            }
        });

        Self {
            drm_device,
            render_device,
            framerate: 60,
            bitrate: std::env::var("BITRATE").unwrap_or_else(|_| "20M".to_string()),
            gop_size: 30,
            codec: VideoCodec::HEVC,
            resolution: VideoResolution::FHD,
        }
    }
}

/// Inicia o processo FFmpeg para captura de tela via kmsgrab + VAAPI.
///
/// ## Pipeline FFmpeg (GPU inteiro):
/// `kmsgrab(DRM) → hwmap(VAAPI) → scale_vaapi(w:h:nv12) → codec_vaapi → Annex-B → stdout`
///
/// Retorna `(Child, ChildStdout)` — o caller deve manter `Child` vivo.
pub fn spawn_ffmpeg(config: &CaptureConfig) -> Result<(Child, ChildStdout)> {
    let (width, height) = config.resolution.dimensions();
    // Pipeline: mantém frame na GPU via VAAPI, escala para a resolução desejada e converte cor para nv12 na GPU
    let vf = format!("hwmap=derive_device=vaapi,scale_vaapi=w={}:h={}:format=nv12", width, height);

    info!(
        "🎬 Iniciando FFmpeg (VAAPI): kmsgrab device={} render={} res={} ({}x{}) fps={} bitrate={} gop={} codec={:?}",
        config.drm_device, config.render_device, config.resolution.name(), width, height, config.framerate, config.bitrate, config.gop_size, config.codec
    );

    let mut ffmpeg_args = vec![
        "-hide_banner".to_string(),
        "-loglevel".to_string(), "warning".to_string(),
        "-threads".to_string(), "1".to_string(),
        "-filter_threads".to_string(), "1".to_string(),
        "-fflags".to_string(), "+nobuffer+flush_packets".to_string(),
        "-flags".to_string(), "low_delay".to_string(),
        // ── Input: kmsgrab DRM/KMS ────────────────────────────────────────────
        "-f".to_string(), "kmsgrab".to_string(),
        "-device".to_string(), config.drm_device.clone(),
        "-framerate".to_string(), config.framerate.to_string(),
        "-i".to_string(), config.drm_device.clone(),
        // ── Filtros GPU ───────────────────────────────────────────────────────
        "-vf".to_string(), vf,
    ];

    match config.codec {
        VideoCodec::H264 => {
            let h264_level = match config.resolution {
                VideoResolution::FHD => "41",
                VideoResolution::QHD => "51",
                VideoResolution::UHD => "52",
            };

            ffmpeg_args.extend([
                "-c:v".to_string(), "h264_vaapi".to_string(),
                "-profile:v".to_string(), "constrained_baseline".to_string(),
                "-level".to_string(), h264_level.to_string(),
                "-bf".to_string(), "0".to_string(),
                "-async_depth".to_string(), "1".to_string(),
                "-rc_mode".to_string(), "CQP".to_string(),
                "-qp".to_string(), "22".to_string(),
                "-g".to_string(), config.gop_size.to_string(),
                "-aud".to_string(), "1".to_string(),
                "-sei".to_string(), "0".to_string(),
                "-flush_packets".to_string(), "1".to_string(),
                "-r".to_string(), config.framerate.to_string(),
                "-an".to_string(),
                "-f".to_string(), "h264".to_string(),
                "pipe:1".to_string(),
            ]);
        }
        VideoCodec::HEVC => {
            ffmpeg_args.extend([
                "-c:v".to_string(), "hevc_vaapi".to_string(),
                "-profile:v".to_string(), "main".to_string(),
                "-bf".to_string(), "0".to_string(),
                "-async_depth".to_string(), "1".to_string(),
                "-rc_mode".to_string(), "CQP".to_string(),
                "-qp".to_string(), "22".to_string(),
                "-g".to_string(), config.gop_size.to_string(),
                "-aud".to_string(), "1".to_string(),
                "-sei".to_string(), "0".to_string(),
                "-flush_packets".to_string(), "1".to_string(),
                "-r".to_string(), config.framerate.to_string(),
                "-an".to_string(),
                "-f".to_string(), "hevc".to_string(),
                "pipe:1".to_string(),
            ]);
        }
        VideoCodec::AV1 => {
            return Err(anyhow::anyhow!("O codec AV1 não é suportado pelo hardware deste servidor. Por favor, use H.264 ou HEVC."));
        }
    }
 
    let mut cmd = Command::new("ffmpeg");
    cmd.args(&ffmpeg_args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true);

    // No Fedora Silverblue ou distribuições imutáveis, drivers freeworld (com suporte a encoding proprietário)
    // são frequentemente colocados em /usr/local/lib64/dri ou /usr/lib64/dri-freeworld.
    // Garantimos que o FFmpeg encontre o driver mesmo se executado via sudo sem -E.
    if std::env::var("LIBVA_DRIVERS_PATH").is_err() {
        for path in &["/usr/local/lib64/dri", "/usr/lib64/dri-freeworld"] {
            if std::path::Path::new(path).exists() {
                cmd.env("LIBVA_DRIVERS_PATH", path);
                break;
            }
        }
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!("O binário 'ffmpeg' não foi encontrado no PATH do sistema. Certifique-se de que o FFmpeg está instalado nesta máquina ou container (ex: sudo dnf install ffmpeg ou sudo apt install ffmpeg). Erro: {}", e)
            } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                anyhow::anyhow!("Permissão negada ao tentar executar o binário FFmpeg (os error 13). Erro: {}", e)
            } else {
                anyhow::anyhow!("Falha ao iniciar processo ffmpeg: {}. Verifique se o binário está disponível.", e)
            }
        })?;
 
    let stdout = child
        .stdout
        .take()
        .context("FFmpeg não retornou stdout")?;
 
    Ok((child, stdout))
}
