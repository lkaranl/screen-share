use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{info, error, warn};
use anyhow::Result;

mod capture;
mod input;
mod rtp;
mod fec;
mod udp_sender;
mod nal_extractor;

use capture::CaptureConfig;
use fec::FecEncoder;
use nal_extractor::NalExtractor;
use udp_sender::UdpSender;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "server=info".to_string()))
        .init();

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    let mut codec = capture::VideoCodec::HEVC; // Padrão HEVC de alta performance
    if let Some(pos) = args.iter().position(|x| x == "--codec") {
        if pos + 1 < args.len() {
            match args[pos + 1].to_lowercase().as_str() {
                "hevc" | "h265" => codec = capture::VideoCodec::HEVC,
                "av1" => codec = capture::VideoCodec::AV1,
                "h264" => codec = capture::VideoCodec::H264,
                other => {
                    warn!("⚠️ Codec desconhecido '{}', usando padrão HEVC", other);
                }
            }
        }
    }

    let mut resolution = capture::VideoResolution::FHD;
    if let Some(pos) = args.iter().position(|x| x == "--res" || x == "--resolution") {
        if pos + 1 < args.len() {
            match args[pos + 1].to_lowercase().as_str() {
                "4k" | "2160p" | "uhd" => resolution = capture::VideoResolution::UHD,
                "2k" | "1440p" | "qhd" => resolution = capture::VideoResolution::QHD,
                "1080p" | "fhd" => resolution = capture::VideoResolution::FHD,
                other => {
                    warn!("⚠️ Resolução desconhecida '{}', usando padrão 1080p", other);
                }
            }
        }
    }

    // Start input handler (uinput virtual devices)
    let input_tx = input::start_input_handler()?;
    info!("✅ Dispositivos virtuais de input criados (mouse + teclado)");

    // Spawn Input/Control TCP Server (Porta 5001)
    let input_tx_clone = input_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = run_control_server(input_tx_clone).await {
            error!("Erro no servidor de controle: {}", e);
        }
    });

    // Run Video UDP + FEC Server (Porta 5000)
    run_video_udp_server(codec, resolution).await?;

    Ok(())
}

async fn run_video_udp_server(default_codec: capture::VideoCodec, default_resolution: capture::VideoResolution) -> Result<()> {
    let udp_sender = Arc::new(UdpSender::bind(5000).await?);
    let fec_encoder = Arc::new(FecEncoder::new(20)); // 20% de tolerância a perdas (Reed-Solomon)

    // Handshake de sessão exclusivamente via TCP na porta 5000
    let tcp_listener = TcpListener::bind("0.0.0.0:5000").await?;
    info!("🎥 Servidor de Vídeo pronto | Codec: {:?} | Resolução padrão: {} | Aguardando cliente TCP na porta 5000", default_codec, default_resolution.name());

    // Canal de cancelamento da sessão ativa (apenas um FFmpeg por vez)
    let mut session_cancel: Option<tokio::sync::oneshot::Sender<()>> = None;

    loop {
        match tcp_listener.accept().await {
            Ok((mut socket, client_addr)) => {
                info!("🔗 Handshake TCP de sessão de {}", client_addr);
                let _ = socket.set_nodelay(true);

                // Lê dados do handshake (2 bytes porta + 1 byte codec + 1 byte resolução opcionais)
                let mut handshake_buf = [0u8; 4];
                let mut active_codec = default_codec;
                let mut active_resolution = default_resolution;
                let client_video_port = match tokio::io::AsyncReadExt::read(&mut socket, &mut handshake_buf).await {
                    Ok(n) if n >= 2 => {
                        let port = u16::from_be_bytes([handshake_buf[0], handshake_buf[1]]);
                        if n >= 3 {
                            match handshake_buf[2] {
                                0 => {
                                    active_codec = capture::VideoCodec::H264;
                                    info!("📦 Cliente solicitou codec H.264 no handshake");
                                }
                                1 => {
                                    active_codec = capture::VideoCodec::HEVC;
                                    info!("📦 Cliente solicitou codec HEVC no handshake");
                                }
                                other => {
                                    warn!("⚠️ Código de codec desconhecido ({}) no handshake, usando padrão {:?}", other, default_codec);
                                }
                            }
                        }
                        if n >= 4 {
                            match handshake_buf[3] {
                                0 => {
                                    active_resolution = capture::VideoResolution::FHD;
                                    info!("📐 Cliente solicitou resolução 1080p (FHD) no handshake");
                                }
                                1 => {
                                    active_resolution = capture::VideoResolution::QHD;
                                    info!("📐 Cliente solicitou resolução 2K (1440p) no handshake");
                                }
                                2 => {
                                    active_resolution = capture::VideoResolution::UHD;
                                    info!("📐 Cliente solicitou resolução 4K (2160p) no handshake");
                                }
                                other => {
                                    warn!("⚠️ Código de resolução desconhecido ({}) no handshake, usando padrão {:?}", other, default_resolution);
                                }
                            }
                        }
                        port
                    }
                    _ => {
                        warn!("⚠️ Cliente conectou mas não enviou a porta UDP — usando porta 50000 padrão");
                        50000u16
                    }
                };

                let codec_id = match active_codec {
                    capture::VideoCodec::H264 => 0u8,
                    capture::VideoCodec::HEVC => 1u8,
                    capture::VideoCodec::AV1 => 2u8,
                };

                let client_udp_target = SocketAddr::new(client_addr.ip(), client_video_port);
                info!("🚀 Iniciando transmissão UDP de vídeo ({:?}, {}) para {} (porta {})", active_codec, active_resolution.name(), client_udp_target, client_video_port);

                // Cancela a sessão anterior se existir
                if let Some(tx) = session_cancel.take() {
                    info!("🔄 Novo cliente — encerrando sessão anterior...");
                    let _ = tx.send(());
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }

                let mut config = CaptureConfig::default();
                config.codec = active_codec;
                config.resolution = active_resolution;

                match capture::spawn_ffmpeg(&config) {
                    Ok((mut child, mut stdout)) => {
                        info!("🎬 FFmpeg iniciado ({:?}), transmitindo via RTP/UDP + FEC...", active_codec);

                        let udp_sender_clone = udp_sender.clone();
                        let fec_encoder_clone = fec_encoder.clone();
                        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();
                        session_cancel = Some(cancel_tx);

                        tokio::spawn(async move {
                            let mut extractor = NalExtractor::new(active_codec);
                            let mut buf = [0u8; 16384];
                            let mut frame_counter: u32 = 0;

                            loop {
                                tokio::select! {
                                    _ = &mut cancel_rx => {
                                        info!("🛑 Sessão cancelada por novo cliente.");
                                        break;
                                    }
                                    result = tokio::io::AsyncReadExt::read(&mut stdout, &mut buf) => {
                                        match result {
                                            Ok(0) => break,
                                            Ok(n) => {
                                                let frames = extractor.push_bytes(&buf[..n]);
                                                for frame_nal in frames {
                                                    frame_counter = frame_counter.wrapping_add(1);
                                                    if let Ok(packets) = fec_encoder_clone.encode_frame(frame_counter, codec_id, &frame_nal) {
                                                        let _ = udp_sender_clone.send_frame_packets(&packets, client_udp_target).await;
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                warn!("⚠️ Erro na leitura do stream de vídeo: {}", e);
                                                break;
                                            }
                                        }
                                    }
                                }
                            }

                            info!("🛑 Encerrando sessão FFmpeg...");
                            let _ = child.kill().await;
                            let _ = child.wait().await;
                        });
                    }
                    Err(e) => {
                        error!("❌ Falha ao iniciar FFmpeg: {:#}", e);
                    }
                }
            }
            Err(e) => {
                error!("❌ Erro ao aceitar conexão TCP: {}", e);
            }
        }
    }
}

async fn run_control_server(input_tx: input::InputSender) -> Result<()> {
    let listener = TcpListener::bind("0.0.0.0:5001").await?;
    info!("🎮 Servidor de Controle (TCP) rodando na porta 5001");

    loop {
        match listener.accept().await {
            Ok((socket, addr)) => {
                info!("🔗 Cliente conectado no canal de Controle: {}", addr);
                let _ = socket.set_nodelay(true);
                let input_tx = input_tx.clone();

                tokio::spawn(async move {
                    let (read_half, mut write_half) = tokio::io::split(socket);
                    let mut reader = BufReader::new(read_half);
                    let mut line = String::new();

                    loop {
                        line.clear();
                        match reader.read_line(&mut line).await {
                            Ok(0) => {
                                info!("⏹️  Cliente de controle desconectado.");
                                break;
                            }
                            Ok(_) => {
                                match serde_json::from_str::<input::InputCommand>(&line) {
                                    Ok(cmd) => {
                                        match cmd {
                                            input::InputCommand::Ping { timestamp } => {
                                                let resp = input::ControlResponse::Pong { timestamp };
                                                if let Ok(mut resp_json) = serde_json::to_string(&resp) {
                                                    resp_json.push('\n');
                                                    let _ = write_half.write_all(resp_json.as_bytes()).await;
                                                }
                                            }
                                            input::InputCommand::ClipboardRequest => {
                                                if let Ok(text) = input::get_remote_clipboard() {
                                                    let resp = input::ControlResponse::ClipboardSync { text };
                                                    if let Ok(mut resp_json) = serde_json::to_string(&resp) {
                                                        resp_json.push('\n');
                                                        let _ = write_half.write_all(resp_json.as_bytes()).await;
                                                    }
                                                }
                                            }
                                            input::InputCommand::ClipboardPaste { text } => {
                                                let _ = input::set_remote_clipboard(&text);
                                                let _ = input_tx.send(input::InputCommand::Key { code: 29, pressed: true }).await;
                                                tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                                                let _ = input_tx.send(input::InputCommand::Key { code: 47, pressed: true }).await;
                                                tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                                                let _ = input_tx.send(input::InputCommand::Key { code: 47, pressed: false }).await;
                                                tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                                                let _ = input_tx.send(input::InputCommand::Key { code: 29, pressed: false }).await;
                                            }
                                            other => {
                                                let _ = input_tx.send(other).await;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!("⚠️  Comando JSON inválido: {} | Linha: {}", e, line);
                                    }
                                }
                            }
                            Err(e) => {
                                error!("❌ Erro ao ler do socket de controle: {}", e);
                                break;
                            }
                        }
                    }
                });
            }
            Err(e) => {
                error!("❌ Erro ao aceitar conexão TCP de controle: {}", e);
            }
        }
    }
}
