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
    run_video_udp_server(codec).await?;

    Ok(())
}

async fn run_video_udp_server(codec: capture::VideoCodec) -> Result<()> {
    let udp_sender = Arc::new(UdpSender::bind(5000).await?);
    let codec_id = match codec {
        capture::VideoCodec::H264 => 0u8,
        capture::VideoCodec::HEVC => 1u8,
        capture::VideoCodec::AV1 => 2u8,
    };

    let fec_encoder = Arc::new(FecEncoder::new(20)); // 20% de tolerância a perdas (Reed-Solomon)

    // Socket TCP 5000 para sincronismo de handshake e descoberta do cliente
    let tcp_listener = TcpListener::bind("0.0.0.0:5000").await?;
    info!("🎥 Servidor de Vídeo (RTP/UDP + FEC + Handshake) pronto na porta 5000 | Codec: {:?}", codec);

    loop {
        match tcp_listener.accept().await {
            Ok((mut socket, client_addr)) => {
                info!("🔗 Cliente conectado para sessão de vídeo UDP: {}", client_addr);
                let _ = socket.set_nodelay(true);

                // O cliente receberá o fluxo UDP na mesma porta 5000 em seu IP
                let client_udp_target = SocketAddr::new(client_addr.ip(), 5000);
                info!("🚀 Iniciando transmissão UDP de vídeo para {}", client_udp_target);

                let mut config = CaptureConfig::default();
                config.codec = codec;

                match capture::spawn_ffmpeg(&config) {
                    Ok((mut child, mut stdout)) => {
                        info!("🎬 FFmpeg iniciado ({:?}), transmitindo via RTP/UDP + FEC...", codec);

                        let udp_sender_clone = udp_sender.clone();
                        let fec_encoder_clone = fec_encoder.clone();

                        tokio::spawn(async move {
                            let mut extractor = NalExtractor::new(codec_id);
                            let mut buf = [0u8; 16384];
                            let mut frame_counter: u32 = 0;

                            loop {
                                match tokio::io::AsyncReadExt::read(&mut stdout, &mut buf).await {
                                    Ok(0) => break, // EOF
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

                            info!("🛑 Encerrando sessão FFmpeg...");
                            let _ = child.kill().await;
                            let _ = child.wait().await;
                        });
                    }
                    Err(e) => {
                        error!("❌ Falha ao iniciar FFmpeg: {}", e);
                    }
                }
            }
            Err(e) => {
                error!("❌ Erro ao aceitar conexão TCP de handshake de vídeo: {}", e);
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

                // Detecção das variáveis da sessão do usuário logado para interagir com o X11 (unclutter)
                let username = std::env::var("SUDO_USER")
                    .or_else(|_| std::env::var("USER"))
                    .unwrap_or_else(|_| "servidor".to_string());
                let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());
                let home_dir = if username == "root" {
                    "/root".to_string()
                } else {
                    format!("/home/{}", username)
                };
                let xauthority = format!("{}/.Xauthority", home_dir);

                // Executa unclutter como o usuário comum para ocultar o cursor nativo no X11
                let unclutter_child = if username != "root" {
                    std::process::Command::new("sudo")
                        .args(&["-u", &username, "env", &format!("DISPLAY={}", display), &format!("XAUTHORITY={}", xauthority), "unclutter", "-idle", "0"])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                        .ok()
                } else {
                    std::process::Command::new("unclutter")
                        .args(&["-idle", "0"])
                        .env("DISPLAY", &display)
                        .env("XAUTHORITY", &xauthority)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                        .ok()
                };

                if unclutter_child.is_some() {
                    info!("🖱️  Ocultando cursor do mouse remoto usando unclutter...");
                }

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

                    // Reexibe o cursor remoto do mouse matando o unclutter
                    if let Some(mut child) = unclutter_child {
                        info!("🖱️  Reexibindo cursor do mouse remoto...");
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                });
            }
            Err(e) => {
                error!("❌ Erro ao aceitar conexão TCP de controle: {}", e);
            }
        }
    }
}
