use tokio::net::TcpListener;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{info, error, warn};
use anyhow::Result;

mod capture;
mod input;

use capture::CaptureConfig;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "server=info".to_string()))
        .init();

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    let mut codec = capture::VideoCodec::H264;
    if let Some(pos) = args.iter().position(|x| x == "--codec") {
        if pos + 1 < args.len() {
            match args[pos + 1].to_lowercase().as_str() {
                "hevc" | "h265" => codec = capture::VideoCodec::HEVC,
                "av1" => codec = capture::VideoCodec::AV1,
                "h264" => codec = capture::VideoCodec::H264,
                other => {
                    warn!("⚠️ Codec desconhecido '{}', usando padrão H.264", other);
                }
            }
        }
    }

    // Start input handler (uinput virtual devices)
    let input_tx = input::start_input_handler()?;
    info!("✅ Dispositivos virtuais de input criados (mouse + teclado)");

    // Run Control Server (which spawns the UDP video stream per client)
    run_control_server(input_tx, codec).await?;

    Ok(())
}

async fn run_control_server(input_tx: input::InputSender, codec: capture::VideoCodec) -> Result<()> {
    let listener = TcpListener::bind("0.0.0.0:5001").await?;
    info!("🎮 Servidor de Controle (TCP) e Vídeo (UDP) rodando na porta 5001");

    loop {
        match listener.accept().await {
            Ok((socket, addr)) => {
                info!("🔗 Cliente conectado no canal de Controle: {}", addr);
                let _ = socket.set_nodelay(true);
                let input_tx = input_tx.clone();
                let client_ip = addr.ip().to_string();

                // Iniciar FFmpeg transmitindo via UDP diretamente para o IP do cliente conectado
                let mut config = CaptureConfig::default();
                config.codec = codec;
                let output_url = format!("udp://{}:5000?pkt_size=1316&buffer_size=65535", client_ip);

                let mut ffmpeg_child = match capture::spawn_ffmpeg(&config, &output_url) {
                    Ok(child) => {
                        info!("🎥 Stream UDP de vídeo iniciado direcionado para {}:5000", client_ip);
                        Some(child)
                    }
                    Err(e) => {
                        error!("❌ Falha ao iniciar FFmpeg UDP: {}", e);
                        None
                    }
                };

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

                    // Encerra o stream de vídeo associado
                    if let Some(mut child) = ffmpeg_child.take() {
                        info!("🛑 Encerrando stream de vídeo UDP...");
                        let _ = child.kill().await;
                        let _ = child.wait().await;
                        info!("✅ Stream UDP finalizado.");
                    }
                });
            }
            Err(e) => {
                error!("❌ Erro ao aceitar conexão TCP de controle: {}", e);
            }
        }
    }
}
