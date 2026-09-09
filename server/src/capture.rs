use anyhow::{Context, Result};
use tokio::process::{Child, ChildStdout, Command};
use tracing::{info, warn};

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

/// Representa um par de dispositivo de display DRM e dispositivo de render VAAPI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrmDevicePair {
    pub drm_device: String,
    pub render_device: String,
    pub active_connectors: usize,
}

/// Detecta os dispositivos DRM e nós de render VAAPI disponíveis no sistema,
/// priorizando placas que possuem saídas com monitor ativamente conectado.
pub fn detect_drm_devices() -> Vec<DrmDevicePair> {
    let mut pairs = Vec::new();

    // 1. Se DRM_DEVICE foi definido via variável de ambiente, honra com prioridade máxima.
    if let Ok(env_drm) = std::env::var("DRM_DEVICE") {
        let env_render = std::env::var("RENDER_DEVICE").unwrap_or_else(|_| {
            find_matching_render_device(&env_drm).unwrap_or_else(|| "/dev/dri/renderD128".to_string())
        });
        pairs.push(DrmDevicePair {
            drm_device: env_drm,
            render_device: env_render,
            active_connectors: 999,
        });
    }

    // 2. Varrer /sys/class/drm procurando placas ativas e seus conectores (status == "connected")
    let sys_drm = std::path::Path::new("/sys/class/drm");
    if sys_drm.exists() && sys_drm.is_dir() {
        if let Ok(entries) = std::fs::read_dir(sys_drm) {
            let mut card_connectors: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            let mut available_cards = std::collections::BTreeSet::new();

            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                // Exemplo: "card0", "card1"
                if name.starts_with("card") && !name.contains('-') {
                    let dev_path = format!("/dev/dri/{}", name);
                    if std::path::Path::new(&dev_path).exists() {
                        available_cards.insert(name.clone());
                        card_connectors.entry(name).or_insert(0);
                    }
                } else if name.starts_with("card") && name.contains('-') {
                    // Exemplo: "card0-DP-1", "card1-HDMI-A-1", "card0-eDP-1"
                    if let Some(card_prefix) = name.split('-').next() {
                        let status_file = entry.path().join("status");
                        if let Ok(status) = std::fs::read_to_string(status_file) {
                            if status.trim().eq_ignore_ascii_case("connected") {
                                *card_connectors.entry(card_prefix.to_string()).or_insert(0) += 1;
                                available_cards.insert(card_prefix.to_string());
                            }
                        }
                    }
                }
            }

            for card_name in available_cards {
                let drm_dev = format!("/dev/dri/{}", card_name);
                let render_dev = find_matching_render_device(&drm_dev)
                    .unwrap_or_else(|| {
                        if std::path::Path::new("/dev/dri/renderD128").exists() {
                            "/dev/dri/renderD128".to_string()
                        } else {
                            "/dev/dri/renderD129".to_string()
                        }
                    });
                let connectors = card_connectors.get(&card_name).copied().unwrap_or(0);

                if !pairs.iter().any(|p| p.drm_device == drm_dev) {
                    pairs.push(DrmDevicePair {
                        drm_device: drm_dev,
                        render_device: render_dev,
                        active_connectors: connectors,
                    });
                }
            }
        }
    }

    // 3. Fallback estático em /dev/dri caso /sys/class/drm não tenha listado
    for default_card in &["/dev/dri/card0", "/dev/dri/card1", "/dev/dri/card2"] {
        if std::path::Path::new(default_card).exists() && !pairs.iter().any(|p| p.drm_device == *default_card) {
            let render_dev = find_matching_render_device(default_card)
                .unwrap_or_else(|| "/dev/dri/renderD128".to_string());
            pairs.push(DrmDevicePair {
                drm_device: default_card.to_string(),
                render_device: render_dev,
                active_connectors: 0,
            });
        }
    }

    // Ordenar: placas com monitores ativos ("connected") primeiro, depois por ordem de nome
    pairs.sort_by(|a, b| {
        b.active_connectors.cmp(&a.active_connectors)
            .then_with(|| a.drm_device.cmp(&b.drm_device))
    });

    pairs
}

/// Encontra o nó de render (/dev/dri/renderD*) correspondente ao dispositivo DRM/KMS (/dev/dri/card*),
/// comparando o caminho PCI/dispositivo do kernel no sysfs.
fn find_matching_render_device(drm_dev: &str) -> Option<String> {
    let card_name = std::path::Path::new(drm_dev).file_name()?.to_string_lossy();
    let card_device_symlink = format!("/sys/class/drm/{}/device", card_name);
    let card_pci_target = std::fs::canonicalize(card_device_symlink).ok();

    if let Some(target) = card_pci_target {
        let sys_drm = std::path::Path::new("/sys/class/drm");
        if let Ok(entries) = std::fs::read_dir(sys_drm) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("renderD") {
                    let render_device_symlink = entry.path().join("device");
                    if let Ok(render_pci_target) = std::fs::canonicalize(render_device_symlink) {
                        if render_pci_target == target {
                            let render_node = format!("/dev/dri/{}", name);
                            if std::path::Path::new(&render_node).exists() {
                                return Some(render_node);
                            }
                        }
                    }
                }
            }
        }
    }

    // Heurística de fallback caso o sysfs não permita canonicalize
    if card_name == "card0" && std::path::Path::new("/dev/dri/renderD128").exists() {
        Some("/dev/dri/renderD128".to_string())
    } else if card_name == "card1" && std::path::Path::new("/dev/dri/renderD129").exists() {
        Some("/dev/dri/renderD129".to_string())
    } else if std::path::Path::new("/dev/dri/renderD128").exists() {
        Some("/dev/dri/renderD128".to_string())
    } else {
        None
    }
}

/// Configuração do pipeline de captura de tela.
#[derive(Debug, Clone)]
pub struct CaptureConfig {
    /// Dispositivo DRM/KMS (ex: "/dev/dri/card0")
    pub drm_device: String,
    /// Dispositivo de render VAAPI (ex: "/dev/dri/renderD128")
    pub render_device: String,
    /// Frames por segundo
    pub framerate: u32,
    /// Bitrate alvo (ex: "20M")
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
        let detected = detect_drm_devices();
        let (drm_device, render_device) = if let Some(best) = detected.first() {
            (best.drm_device.clone(), best.render_device.clone())
        } else {
            ("/dev/dri/card0".to_string(), "/dev/dri/renderD128".to_string())
        };

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

/// Cria e inicia uma instância única do processo FFmpeg com as configurações fornecidas.
fn spawn_single_ffmpeg(config: &CaptureConfig) -> Result<(Child, ChildStdout)> {
    let (width, height) = config.resolution.dimensions();
    // Pipeline: mantém frame na GPU via VAAPI, escala para a resolução desejada e converte cor para nv12 na GPU
    let vf = format!("hwmap=derive_device=vaapi,scale_vaapi=w={}:h={}:format=nv12", width, height);

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

/// Inicia o processo FFmpeg para captura de tela via kmsgrab + VAAPI.
///
/// Tenta de forma inteligente o dispositivo DRM configurado e realiza fallback
/// automático para outros dispositivos DRM detectados caso o dispositivo inicial
/// falhe (por exemplo, "No usable planes found" ou tela desconectada).
pub async fn spawn_ffmpeg(config: &mut CaptureConfig) -> Result<(Child, ChildStdout)> {
    let detected_candidates = detect_drm_devices();

    let mut candidates_to_try: Vec<(String, String)> = Vec::new();
    // Prioriza o dispositivo atual do config
    candidates_to_try.push((config.drm_device.clone(), config.render_device.clone()));

    // Adiciona os outros candidatos detectados caso ainda não estejam na lista
    for pair in detected_candidates {
        if !candidates_to_try.iter().any(|(d, _)| d == &pair.drm_device) {
            candidates_to_try.push((pair.drm_device, pair.render_device));
        }
    }

    let (width, height) = config.resolution.dimensions();
    let mut last_error = None;
    let mut tested_devices = Vec::new();

    for (drm_dev, render_dev) in candidates_to_try {
        config.drm_device = drm_dev.clone();
        config.render_device = render_dev.clone();
        tested_devices.push(drm_dev.clone());

        info!(
            "🎬 Tentando FFmpeg (VAAPI): kmsgrab device={} render={} res={} ({}x{}) fps={} bitrate={} codec={:?}",
            config.drm_device, config.render_device, config.resolution.name(), width, height, config.framerate, config.bitrate, config.codec
        );

        match spawn_single_ffmpeg(config) {
            Ok((mut child, stdout)) => {
                // Aguarda 150ms para verificar se o FFmpeg inicializou o kmsgrab com sucesso
                // ou se encerrou imediatamente com erro ("No usable planes found", "Invalid argument", etc.)
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;

                match child.try_wait() {
                    Ok(Some(status)) => {
                        warn!(
                            "⚠️ FFmpeg falhou no dispositivo DRM {} (exit status: {:?}). Tentando próximo dispositivo...",
                            drm_dev, status
                        );
                        last_error = Some(anyhow::anyhow!(
                            "Dispositivo DRM {} falhou com status {:?} (sem planos KMS utilizáveis)",
                            drm_dev, status
                        ));
                    }
                    Ok(None) => {
                        info!(
                            "✅ Captura DRM/KMS ativa com sucesso no dispositivo {} (render: {})",
                            config.drm_device, config.render_device
                        );
                        return Ok((child, stdout));
                    }
                    Err(e) => {
                        warn!("⚠️ Erro ao verificar integridade do processo FFmpeg em {}: {}", drm_dev, e);
                        last_error = Some(e.into());
                    }
                }
            }
            Err(e) => {
                warn!("⚠️ Falha ao criar processo FFmpeg para DRM {}: {}", drm_dev, e);
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        anyhow::anyhow!(
            "Nenhum dispositivo DRM/KMS funcional encontrado para captura via kmsgrab. Dispositivos testados: {:?}. Verifique permissões (sudo/grupo video) ou se a tela está ativa.",
            tested_devices
        )
    }))
}
