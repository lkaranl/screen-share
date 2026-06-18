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
            gop_size: 30, // keyframe a cada 1 segundo @ 30fps (mais fácil para browser sincronizar)
        }
    }
}

/// Inicia o processo FFmpeg para captura de tela via kmsgrab + VAAPI.
///
/// ## Pipeline FFmpeg (GPU inteiro):
/// `kmsgrab(DRM) → hwmap(VAAPI) → scale_vaapi(nv12) → h264_vaapi → H.264 Annex-B → stdout`
///
/// Retorna `(Child, ChildStdout)` — o caller deve manter `Child` vivo.
pub fn spawn_ffmpeg(config: &CaptureConfig) -> Result<(Child, ChildStdout)> {
    let gop_str = config.gop_size.to_string();
    let framerate_str = config.framerate.to_string();
    let bitrate = &config.bitrate;

    // Pipeline: mantém frame na GPU via VAAPI
    let vf = "hwmap=derive_device=vaapi,scale_vaapi=w=1280:h=720:format=nv12".to_string();
 
    info!(
        "🎬 Iniciando FFmpeg (VAAPI): kmsgrab device={} render={} fps={} bitrate={}",
        config.drm_device, config.render_device, config.framerate, config.bitrate
    );
 
    let mut child = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel", "warning",
            // ── Hardware VAAPI ────────────────────────────────────────────────────
            "-init_hw_device", &format!("drm=drm:{}", config.render_device),
            "-init_hw_device", "vaapi=va@drm",
            "-filter_hw_device", "va",
            // ── Input: kmsgrab DRM/KMS ────────────────────────────────────────────
            "-f", "kmsgrab",
            "-device", &config.drm_device,
            "-framerate", &framerate_str,
            "-i", &config.drm_device,
            // ── Filtros GPU ───────────────────────────────────────────────────────
            "-vf", &vf,
            // ── Codec H.264 VAAPI ─────────────────────────────────────────────────
            "-c:v", "h264_vaapi",
            // constrained_baseline = máxima compatibilidade com Chrome/Firefox/Safari
            "-profile:v", "constrained_baseline",
            "-level", "31",
            "-b:v", bitrate,
            "-maxrate", bitrate,
            "-bufsize", "1M",
            // Keyframe frequente: a cada 30 frames (1 segundo). Assim o browser
            // consegue iniciar decodificação rapidamente ao se conectar.
            "-g", &gop_str,
            "-force_key_frames", "expr:gte(t,n_forced*1)",
            // dump_extra injeta SPS/PPS em cada IDR frame (in-band headers).
            // Sem isso, browsers frequentemente ficam com tela preta.
            "-bsf:v", "dump_extra=freq=keyframe",
            // ── Sem áudio ─────────────────────────────────────────────────────────
            "-an",
            // ── Saída: H.264 Annex-B para stdout ─────────────────────────────────
            "-f", "h264",
            "pipe:1",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .context("Falha ao iniciar ffmpeg com VAAPI. Verifique se h264_vaapi está disponível.")?;

    let stdout = child
        .stdout
        .take()
        .context("FFmpeg não retornou stdout")?;

    Ok((child, stdout))
}
