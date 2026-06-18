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
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            drm_device: std::env::var("DRM_DEVICE")
                .unwrap_or_else(|_| "/dev/dri/card1".to_string()),
            render_device: std::env::var("RENDER_DEVICE")
                .unwrap_or_else(|_| "/dev/dri/renderD128".to_string()),
            framerate: 30,
            bitrate: "40M".to_string(),
            gop_size: 30,
            codec: VideoCodec::H264,
        }
    }
}

/// Inicia o processo FFmpeg para captura de tela via kmsgrab + VAAPI.
///
/// ## Pipeline FFmpeg (GPU inteiro):
/// `kmsgrab(DRM) → hwmap(VAAPI) → scale_vaapi(nv12) → codec_vaapi → Annex-B → stdout`
///
/// Retorna `(Child, ChildStdout)` — o caller deve manter `Child` vivo.
pub fn spawn_ffmpeg(config: &CaptureConfig) -> Result<(Child, ChildStdout)> {
    let gop_str = config.gop_size.to_string();
    let framerate_str = config.framerate.to_string();
    let bitrate = &config.bitrate;
 
    // Pipeline: mantém frame na GPU via VAAPI, mas sem forçar escala (captura na resolução nativa)
    let vf = "hwmap=derive_device=vaapi,scale_vaapi=format=nv12".to_string();
 
    info!(
        "🎬 Iniciando FFmpeg (VAAPI): kmsgrab device={} render={} fps={} bitrate={} codec={:?}",
        config.drm_device, config.render_device, config.framerate, config.bitrate, config.codec
    );
 
    let mut ffmpeg_args = vec![
        "-hide_banner".to_string(),
        "-loglevel".to_string(), "warning".to_string(),
        // ── Hardware VAAPI ────────────────────────────────────────────────────
        "-init_hw_device".to_string(), format!("drm=drm:{}", config.render_device),
        "-init_hw_device".to_string(), "vaapi=va@drm".to_string(),
        "-filter_hw_device".to_string(), "va".to_string(),
        // ── Input: kmsgrab DRM/KMS ────────────────────────────────────────────
        "-f".to_string(), "kmsgrab".to_string(),
        "-device".to_string(), config.drm_device.clone(),
        "-framerate".to_string(), framerate_str,
        "-i".to_string(), config.drm_device.clone(),
        // ── Filtros GPU ───────────────────────────────────────────────────────
        "-vf".to_string(), vf,
    ];
 
    match config.codec {
        VideoCodec::H264 => {
            ffmpeg_args.extend([
                "-c:v".to_string(), "h264_vaapi".to_string(),
                "-profile:v".to_string(), "constrained_baseline".to_string(),
                "-level".to_string(), "31".to_string(),
                "-b:v".to_string(), bitrate.clone(),
                "-maxrate".to_string(), bitrate.clone(),
                "-bufsize".to_string(), "10M".to_string(),
                "-g".to_string(), gop_str,
                "-force_key_frames".to_string(), "expr:gte(t,n_forced*1)".to_string(),
                "-bsf:v".to_string(), "dump_extra=freq=keyframe".to_string(),
                "-an".to_string(),
                "-f".to_string(), "h264".to_string(),
                "pipe:1".to_string(),
            ]);
        }
        VideoCodec::HEVC => {
            ffmpeg_args.extend([
                "-c:v".to_string(), "hevc_vaapi".to_string(),
                "-bf".to_string(), "0".to_string(),
                "-b:v".to_string(), bitrate.clone(),
                "-maxrate".to_string(), bitrate.clone(),
                "-bufsize".to_string(), "10M".to_string(),
                "-g".to_string(), gop_str,
                "-force_key_frames".to_string(), "expr:gte(t,n_forced*1)".to_string(),
                "-bsf:v".to_string(), "hevc_mp4toannexb".to_string(),
                "-an".to_string(),
                "-f".to_string(), "hevc".to_string(),
                "pipe:1".to_string(),
            ]);
        }
        VideoCodec::AV1 => {
            return Err(anyhow::anyhow!("O codec AV1 não é suportado pelo hardware deste servidor. Por favor, use H.264 ou HEVC."));
        }
    }
 
    let mut child = Command::new("ffmpeg")
        .args(&ffmpeg_args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .context("Falha ao iniciar ffmpeg com VAAPI. Verifique se o codec selecionado está disponível em hardware.")?;
 
    let stdout = child
        .stdout
        .take()
        .context("FFmpeg não retornou stdout")?;
 
    Ok((child, stdout))
}
