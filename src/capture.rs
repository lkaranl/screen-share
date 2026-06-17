use anyhow::{Context, Result};
use tokio::process::{Child, ChildStdout, Command};
use tracing::info;

/// Configuração do pipeline de captura de tela.
#[derive(Debug, Clone)]
pub struct CaptureConfig {
    /// Dispositivo DRM/KMS (ex: "/dev/dri/card1")
    pub drm_device: String,
    /// Dispositivo de render VAAPI (ex: "/dev/dri/renderD128")
    pub render_device: String,
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
                .unwrap_or_else(|_| "/dev/dri/card1".to_string()),
            render_device: std::env::var("RENDER_DEVICE")
                .unwrap_or_else(|_| "/dev/dri/renderD128".to_string()),
            framerate: 30,
            bitrate: "4M".to_string(),
            gop_size: 60, // keyframe a cada 2 segundos @ 30fps
        }
    }
}

/// Inicia o processo FFmpeg para captura de tela via kmsgrab + VAAPI.
///
/// ## Por que VAAPI?
/// O framebuffer do servidor está em formato 10-bit (ABGR2101010 / FourCC: AB30).
/// O pipeline clássico `kmsgrab → hwdownload → libx264` falha porque o FFmpeg 6.1
/// não consegue fazer download de CPU para esse formato.
///
/// ## Pipeline FFmpeg (GPU inteiro):
/// `kmsgrab(DRM) → hwmap(VAAPI) → scale_vaapi(nv12) → h264_vaapi → H.264 → pipe:1`
///
/// Todo o processamento fica na GPU — sem cópia para CPU, sem problema de pixel format.
///
/// Retorna `(Child, ChildStdout)` — o caller deve manter `Child` vivo para o processo continuar.
pub fn spawn_ffmpeg(config: &CaptureConfig) -> Result<(Child, ChildStdout)> {
    let gop_str = config.gop_size.to_string();
    let framerate_str = config.framerate.to_string();
    let bitrate = &config.bitrate;

    // Pipeline de filtros: mantém frame na GPU via VAAPI durante todo o processo
    // hwmap: mapeia o frame DRM para a interface VAAPI
    // scale_vaapi: converte para NV12 (formato exigido pelo h264_vaapi)
    let vf = format!(
        "hwmap=derive_device=vaapi,scale_vaapi=format=nv12"
    );

    info!(
        "🎬 Iniciando FFmpeg (VAAPI): kmsgrab device={} render={} fps={} bitrate={}",
        config.drm_device, config.render_device, config.framerate, config.bitrate
    );

    let mut child = Command::new("ffmpeg")
        .args([
            // Suprime banner do FFmpeg
            "-hide_banner",
            "-loglevel", "warning",
            // ── Inicializa hardware VAAPI via render node ─────────────────────────
            // Passo 1: inicializa o DRM device (kmsgrab vai usar este contexto)
            "-init_hw_device", &format!("drm=drm:{}", config.render_device),
            // Passo 2: inicializa VAAPI derivando do DRM
            "-init_hw_device", "vaapi=va@drm",
            // Informa ao filtro qual device VAAPI usar
            "-hwaccel_device", "va",
            // ── Input: DRM/KMS via kmsgrab ─────────────────────────────────────────
            "-f", "kmsgrab",
            "-device", &config.drm_device,
            "-framerate", &framerate_str,
            "-i", &config.drm_device,
            // ── Filtros de vídeo (na GPU via VAAPI) ───────────────────────────────
            "-vf", &vf,
            // ── Codec: H.264 via VAAPI (encode na GPU) ────────────────────────────
            "-c:v", "h264_vaapi",
            "-b:v", bitrate,
            "-maxrate", bitrate,
            "-bufsize", "2M",
            // ── GOP ────────────────────────────────────────────────────────────────
            "-g", &gop_str,
            // ── Sem áudio/legendas ─────────────────────────────────────────────────
            "-an", "-sn",
            // ── Output: H.264 Annex-B para stdout ─────────────────────────────────
            "-f", "h264",
            "pipe:1",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .context("Falha ao iniciar ffmpeg com VAAPI. Verifique se h264_vaapi está disponível e se o render device existe.")?;

    let stdout = child
        .stdout
        .take()
        .context("FFmpeg não retornou stdout")?;

    Ok((child, stdout))
}
