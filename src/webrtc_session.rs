use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use anyhow::Result;
use bytes::Bytes;
use tokio::io::AsyncReadExt;
use tracing::{error, info, trace, warn};

use webrtc::{
    api::{
        interceptor_registry::register_default_interceptors,
        media_engine::{MediaEngine, MIME_TYPE_H264},
        APIBuilder,
    },
    data_channel::{data_channel_message::DataChannelMessage, RTCDataChannel},
    ice_transport::{
        ice_candidate::{RTCIceCandidate, RTCIceCandidateInit},
        ice_server::RTCIceServer,
    },
    interceptor::registry::Registry,
    media::Sample,
    peer_connection::{
        configuration::RTCConfiguration,
        peer_connection_state::RTCPeerConnectionState,
        RTCPeerConnection,
    },
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::{track_local_static_sample::TrackLocalStaticSample, TrackLocal},
};

use crate::{
    capture::{spawn_ffmpeg, CaptureConfig},
    input::{InputCommand, InputSender},
};

/// Cria uma sessão WebRTC completa:
/// - RTCPeerConnection com track de vídeo H.264
/// - DataChannel handler para input do browser
/// - ICE candidate forwarding via channel
/// - Pipeline FFmpeg → H.264 → WebRTC
///
/// Retorna `(pc, is_connected, ice_outbound_tx)`.
pub async fn create_session(
    input_tx: InputSender,
    ice_outbound_tx: tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<Arc<RTCPeerConnection>> {
    // ── MediaEngine: registra codecs padrão (inclui H.264) ─────────────
    let mut m = MediaEngine::default();
    m.register_default_codecs()?;

    // ── Interceptors: NACK, RTCP reports, etc. ──────────────────────────
    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut m)?;

    // ── API WebRTC ───────────────────────────────────────────────────────
    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_interceptor_registry(registry)
        .build();

    // ── Configuração do PeerConnection ──────────────────────────────────
    let config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            // STUN público do Google para descoberta de endereço NAT
            urls: vec!["stun:stun.l.google.com:19302".to_owned()],
            ..Default::default()
        }],
        ..Default::default()
    };

    let pc = Arc::new(api.new_peer_connection(config).await?);

    // ── Track de vídeo H.264 ─────────────────────────────────────────────
    let video_track = Arc::new(TrackLocalStaticSample::new(
        RTCRtpCodecCapability {
            mime_type: MIME_TYPE_H264.to_owned(),
            clock_rate: 90_000, // clock rate padrão para vídeo
            ..Default::default()
        },
        "video".to_owned(),
        "screen-share".to_owned(),
    ));

    // Adiciona a track ao PeerConnection (servidor envia vídeo)
    let rtp_sender = pc
        .add_track(Arc::clone(&video_track) as Arc<dyn TrackLocal + Send + Sync>)
        .await?;

    // ── RTCP reader: processa PLI (Picture Loss Indication) ─────────────
    // O browser envia PLI quando perde frames — pedindo um novo keyframe.
    // Precisamos ler esses pacotes para não bloquear o canal RTCP.
    tokio::spawn(async move {
        let mut buf = vec![0u8; 1500];
        while let Ok((_, _)) = rtp_sender.read(&mut buf).await {
            // PLI é tratado automaticamente pelo interceptor de NACK.
            // Lemos aqui apenas para não bloquear o buffer.
        }
    });

    // ── Flag para detectar encerramento de conexão ───────────────────────
    let is_active = Arc::new(AtomicBool::new(true));
    let is_active_on_state = Arc::clone(&is_active);

    pc.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
        let is_active = Arc::clone(&is_active_on_state);
        Box::pin(async move {
            info!("🔗 Estado WebRTC: {:?}", state);
            match state {
                RTCPeerConnectionState::Failed
                | RTCPeerConnectionState::Closed
                | RTCPeerConnectionState::Disconnected => {
                    is_active.store(false, Ordering::SeqCst);
                }
                RTCPeerConnectionState::Connected => {
                    info!("✅ WebRTC conectado! Iniciando stream de vídeo.");
                }
                _ => {}
            }
        })
    }));

    // ── ICE candidate handler ────────────────────────────────────────────
    // Quando um novo ICE candidate é descoberto localmente, envia ao browser.
    let ice_tx = ice_outbound_tx.clone();
    pc.on_ice_candidate(Box::new(move |c: Option<RTCIceCandidate>| {
        let ice_tx = ice_tx.clone();
        Box::pin(async move {
            if let Some(c) = c {
                match c.to_json().await {
                    Ok(init) => {
                        let msg = serde_json::json!({
                            "type": "ice_candidate",
                            "candidate": init.candidate,
                            "sdpMid": init.sdp_mid,
                            "sdpMLineIndex": init.sdp_mline_index,
                        })
                        .to_string();
                        let _ = ice_tx.send(msg);
                    }
                    Err(e) => warn!("Falha ao serializar ICE candidate: {}", e),
                }
            }
        })
    }));

    // ── DataChannel handler: input do browser ───────────────────────────
    // O browser cria um DataChannel chamado "input".
    // O servidor recebe aqui os eventos de mouse e teclado.
    let input_tx_dc = input_tx.clone();
    pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
        let input_tx = input_tx_dc.clone();
        Box::pin(async move {
            let label = dc.label().to_owned();
            info!("📨 DataChannel recebido: '{}'", label);

            if label != "input" {
                return;
            }

            dc.on_open(Box::new(|| {
                Box::pin(async { info!("🎮 DataChannel 'input' aberto — controle ativo") })
            }));

            dc.on_message(Box::new(move |msg: DataChannelMessage| {
                let input_tx = input_tx.clone();
                Box::pin(async move {
                    if let Ok(text) = std::str::from_utf8(&msg.data) {
                        handle_input_message(&input_tx, text).await;
                    }
                })
            }));
        })
    }));

    // ── Pipeline de vídeo: FFmpeg → WebRTC ──────────────────────────────
    let video_track_task = Arc::clone(&video_track);
    let is_active_task = Arc::clone(&is_active);
    tokio::spawn(async move {
        if let Err(e) = stream_video(video_track_task, is_active_task).await {
            error!("Erro no pipeline de vídeo: {}", e);
        }
    });

    Ok(pc)
}

/// Processa uma mensagem JSON de input recebida pelo DataChannel.
async fn handle_input_message(input_tx: &InputSender, text: &str) {
    let Ok(event) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };

    let cmd = match event["t"].as_str() {
        Some("mm") => {
            // Mouse move (delta)
            let dx = event["x"].as_i64().unwrap_or(0) as i32;
            let dy = event["y"].as_i64().unwrap_or(0) as i32;
            InputCommand::MouseMove { dx, dy }
        }
        Some("mb") => {
            // Mouse button
            let button = event["b"].as_u64().unwrap_or(0) as u8;
            let pressed = event["d"].as_bool().unwrap_or(false);
            InputCommand::MouseButton { button, pressed }
        }
        Some("mw") => {
            // Mouse scroll
            let dy = event["dy"].as_i64().unwrap_or(0) as i32;
            InputCommand::MouseScroll { dy }
        }
        Some("k") => {
            // Keypress
            let code = event["c"].as_u64().unwrap_or(0) as u16;
            let pressed = event["d"].as_bool().unwrap_or(false);
            InputCommand::Key { code, pressed }
        }
        _ => return,
    };

    if let Err(e) = input_tx.send(cmd).await {
        trace!("Input channel cheio ou fechado: {}", e);
    }
}

/// Pipeline assíncrono: lê H.264 do FFmpeg e envia à WebRTC track.
///
/// O FFmpeg com `-tune zerolatency` faz flush por frame, então cada
/// `read()` retorna aproximadamente 1 frame de dados H.264.
async fn stream_video(
    track: Arc<TrackLocalStaticSample>,
    is_active: Arc<AtomicBool>,
) -> Result<()> {
    let config = CaptureConfig::default();

    // Aguarda um momento para o PeerConnection negociar antes de iniciar FFmpeg
    tokio::time::sleep(Duration::from_millis(500)).await;

    let (_child, mut stdout) = spawn_ffmpeg(&config)?;
    // _child é mantido vivo aqui — quando sair do escopo, o processo FFmpeg morre.

    // Buffer para leitura dos frames H.264 (256KB)
    let mut buf = vec![0u8; 262_144];
    let frame_duration = Duration::from_millis(1000 / config.framerate as u64);

    info!("🎥 Pipeline de vídeo iniciado — aguardando conexão WebRTC...");

    loop {
        // Verifica se a conexão ainda está ativa
        if !is_active.load(Ordering::SeqCst) {
            info!("🛑 Conexão WebRTC encerrada — parando FFmpeg");
            break;
        }

        // Lê dados do FFmpeg com timeout
        match tokio::time::timeout(Duration::from_secs(5), stdout.read(&mut buf)).await {
            Ok(Ok(0)) => {
                warn!("FFmpeg encerrou o stdout (processo terminou)");
                break;
            }
            Ok(Ok(n)) => {
                // Envia o chunk H.264 como um Sample para a WebRTC track.
                // O H264Payloader interno do webrtc-rs cuida da packetização RTP.
                let sample = Sample {
                    data: Bytes::copy_from_slice(&buf[..n]),
                    duration: frame_duration,
                    ..Default::default()
                };
                if let Err(e) = track.write_sample(&sample).await {
                    // Erros aqui são esperados antes da conexão estar estabelecida.
                    trace!("write_sample: {} (normal antes de conectar)", e);
                }
            }
            Ok(Err(e)) => {
                error!("Erro lendo stdout do FFmpeg: {}", e);
                break;
            }
            Err(_) => {
                warn!("Timeout lendo FFmpeg (5s sem dados)");
                break;
            }
        }
    }

    info!("Pipeline de vídeo encerrado");
    Ok(())
}
