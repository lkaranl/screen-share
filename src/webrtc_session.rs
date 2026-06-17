use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use anyhow::Result;
use bytes::Bytes;
use tokio::io::AsyncReadExt;
use tracing::{error, info, warn};

use webrtc::{
    api::{
        interceptor_registry::register_default_interceptors,
        media_engine::{MediaEngine, MIME_TYPE_H264},
        APIBuilder,
    },
    data_channel::{data_channel_message::DataChannelMessage, RTCDataChannel},
    ice_transport::{
        ice_candidate::RTCIceCandidate,
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

/// Cria uma sessão WebRTC completa.
pub async fn create_session(
    input_tx: InputSender,
    ice_outbound_tx: tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<Arc<RTCPeerConnection>> {
    let mut m = MediaEngine::default();
    m.register_default_codecs()?;

    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut m)?;

    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_interceptor_registry(registry)
        .build();

    let config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_owned()],
            ..Default::default()
        }],
        ..Default::default()
    };

    let pc = Arc::new(api.new_peer_connection(config).await?);

    // ── Track de vídeo H.264 via TrackLocalStaticSample ─────────────────
    let video_track = Arc::new(TrackLocalStaticSample::new(
        RTCRtpCodecCapability {
            mime_type: MIME_TYPE_H264.to_owned(),
            clock_rate: 90_000,
            ..Default::default()
        },
        "video".to_owned(),
        "screen-share".to_owned(),
    ));

    let rtp_sender = pc
        .add_track(Arc::clone(&video_track) as Arc<dyn TrackLocal + Send + Sync>)
        .await?;

    tokio::spawn(async move {
        let mut buf = vec![0u8; 1500];
        while let Ok((_, _)) = rtp_sender.read(&mut buf).await {}
    });

    let is_active = Arc::new(AtomicBool::new(true));
    let is_active_state = Arc::clone(&is_active);

    pc.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
        let is_active = Arc::clone(&is_active_state);
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

    let ice_tx = ice_outbound_tx.clone();
    pc.on_ice_candidate(Box::new(move |c: Option<RTCIceCandidate>| {
        let ice_tx = ice_tx.clone();
        Box::pin(async move {
            if let Some(c) = c {
                match c.to_json() {
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

    let video_track_task = Arc::clone(&video_track);
    let is_active_task = Arc::clone(&is_active);
    tokio::spawn(async move {
        if let Err(e) = stream_video(video_track_task, is_active_task).await {
            error!("Erro no pipeline de vídeo: {}", e);
        }
    });

    Ok(pc)
}

async fn handle_input_message(input_tx: &InputSender, text: &str) {
    let Ok(event) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    let cmd = match event["t"].as_str() {
        Some("mm") => InputCommand::MouseMove {
            dx: event["x"].as_i64().unwrap_or(0) as i32,
            dy: event["y"].as_i64().unwrap_or(0) as i32,
        },
        Some("mb") => InputCommand::MouseButton {
            button: event["b"].as_u64().unwrap_or(0) as u8,
            pressed: event["d"].as_bool().unwrap_or(false),
        },
        Some("mw") => InputCommand::MouseScroll {
            dy: event["dy"].as_i64().unwrap_or(0) as i32,
        },
        Some("k") => InputCommand::Key {
            code: event["c"].as_u64().unwrap_or(0) as u16,
            pressed: event["d"].as_bool().unwrap_or(false),
        },
        _ => return,
    };
    if let Err(e) = input_tx.send(cmd).await {
        warn!("Input channel fechado: {}", e);
    }
}

/// Pipeline de vídeo: FFmpeg → RTP UDP → Sample → WebRTC.
///
/// Nesta versão o FFmpeg envia RTP via UDP para a porta 5004.
/// Cada datagrama UDP contém exatamente um pacote RTP do FFmpeg
/// com o payload H.264 fragmentado (FU-A ou STAP-A).
///
/// Para o `TrackLocalStaticSample` funcionar corretamente, precisamos
/// entregar NAL units completas (frames H.264 completos). Por isso,
/// nesta abordagem lemos do stdout do FFmpeg usando output de formato h264,
/// mas com framing correto: detectamos start codes Annex-B e separamos
/// cada NALU individualmente.
async fn stream_video(
    track: Arc<TrackLocalStaticSample>,
    is_active: Arc<AtomicBool>,
) -> Result<()> {
    let config = CaptureConfig::default();

    tokio::time::sleep(Duration::from_millis(500)).await;

    let (_child, mut stdout) = spawn_ffmpeg(&config)?;
    let frame_duration = Duration::from_millis(1000 / config.framerate as u64);

    info!("🎥 Pipeline de vídeo iniciado — lendo frames H.264 do FFmpeg...");

    // Lemos o stream Annex-B do FFmpeg e separamos NAL units pelo start code.
    // O formato Annex-B usa `00 00 01` ou `00 00 00 01` como delimitador.
    //
    // Estratégia: acumulamos bytes num buffer e enviamos a cada vez que
    // detectamos o início de um novo NAL unit (start code), entregando o
    // NAL unit anterior completo ao write_sample.
    let mut ring = Vec::<u8>::with_capacity(1 << 20); // 1MB
    let mut tmp = vec![0u8; 65536];
    let mut frame_count = 0u64;

    // Função auxiliar: encontra o próximo start code no buffer a partir de `from`.
    fn find_start_code(buf: &[u8], from: usize) -> Option<usize> {
        if buf.len() < from + 4 {
            return None;
        }
        for i in from..buf.len().saturating_sub(3) {
            if buf[i] == 0 && buf[i+1] == 0 {
                if buf[i+2] == 1 {
                    return Some(i);   // 00 00 01
                }
                if i + 3 < buf.len() && buf[i+2] == 0 && buf[i+3] == 1 {
                    return Some(i);   // 00 00 00 01
                }
            }
        }
        None
    }

    loop {
        if !is_active.load(Ordering::SeqCst) {
            info!("🛑 Conexão encerrada — parando FFmpeg");
            break;
        }

        match tokio::time::timeout(Duration::from_secs(5), stdout.read(&mut tmp)).await {
            Ok(Ok(0)) => {
                warn!("FFmpeg encerrou o stdout");
                break;
            }
            Ok(Ok(n)) => {
                ring.extend_from_slice(&tmp[..n]);

                // Procura pares de start codes para extrair NAL units completas
                let mut search_from = 0usize;
                loop {
                    let first = match find_start_code(&ring, search_from) {
                        Some(p) => p,
                        None => break,
                    };

                    // Avança past o start code para procurar o próximo
                    let sc_len = if ring.get(first + 2) == Some(&1) { 3 } else { 4 };
                    let next = match find_start_code(&ring, first + sc_len) {
                        Some(p) => p,
                        None => break, // Ainda não temos o fim deste NAL unit
                    };

                    // Determina o tamanho do start code (3 ou 4 bytes)
                    let sc_len = if ring.get(first + 2) == Some(&1) { 3 } else { 4 };
                    
                    // Extrai o NAL unit puro (sem o start code)
                    let nalu_data = &ring[first + sc_len..next];
                    if nalu_data.is_empty() {
                        search_from = next;
                        continue;
                    }

                    let nal_type = nalu_data[0] & 0x1F;
                    let nalu_len = nalu_data.len();
                    
                    // Logs detalhados de diagnóstico
                    match nal_type {
                        7 => info!("🔑 [SPS] detectado: {} bytes, prefixo: {:02x?}", nalu_len, &nalu_data[..std::cmp::min(5, nalu_len)]),
                        8 => info!("🔑 [PPS] detectado: {} bytes, prefixo: {:02x?}", nalu_len, &nalu_data[..std::cmp::min(5, nalu_len)]),
                        5 => info!("🔑 [Keyframe IDR] detectado: {} bytes, prefixo: {:02x?}", nalu_len, &nalu_data[..std::cmp::min(5, nalu_len)]),
                        _ => {
                            if frame_count % 90 == 0 {
                                info!("📦 NALU comum (tipo {}): {} bytes, prefixo: {:02x?}", nal_type, nalu_len, &nalu_data[..std::cmp::min(5, nalu_len)]);
                            }
                        }
                    }

                    let nalu = Bytes::copy_from_slice(nalu_data);
                    search_from = next;

                    frame_count += 1;
                    if frame_count % 90 == 0 {
                        info!("📊 {} NAL units processados no total", frame_count);
                    }

                    // No WebRTC, metadados (SPS, PPS, SEI) devem ser aplicados no mesmo
                    // timestamp RTP do frame de vídeo correspondente. Definimos a duração
                    // como zero para que o webrtc-rs envie-os no mesmo timestamp RTP.
                    let sample_duration = match nal_type {
                        7 | 8 | 6 => Duration::from_secs(0),
                        _ => frame_duration,
                    };

                    let sample = Sample {
                        data: nalu,
                        duration: sample_duration,
                        ..Default::default()
                    };

                    if let Err(e) = track.write_sample(&sample).await {
                        error!("❌ Erro no write_sample (NALU tipo {}): {}", nal_type, e);
                    }
                }

                // Remove os dados já processados, mantém o restante
                if search_from > 0 {
                    ring.drain(..search_from);
                }

                // Segurança: se o buffer crescer demais, descartar
                if ring.len() > 4 * 1024 * 1024 {
                    warn!("Buffer cresceu demais ({} KB) — descartando", ring.len() / 1024);
                    ring.clear();
                }
            }
            Ok(Err(e)) => {
                error!("Erro lendo stdout do FFmpeg: {}", e);
                break;
            }
            Err(_) => {
                warn!("Timeout: 5s sem dados do FFmpeg");
                break;
            }
        }
    }

    info!("Pipeline de vídeo encerrado");
    Ok(())
}
