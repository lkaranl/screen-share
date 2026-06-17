use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncBufReadExt, BufReader};
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

    // Start input handler (uinput virtual devices)
    let input_tx = input::start_input_handler()?;
    info!("✅ Dispositivos virtuais de input criados (mouse + teclado)");

    // Spawn Input/Control TCP Server
    let input_tx_clone = input_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = run_control_server(input_tx_clone).await {
            error!("Erro no servidor de controle: {}", e);
        }
    });

    // Run Video TCP Server on main thread
    run_video_server().await?;

    Ok(())
}

async fn run_video_server() -> Result<()> {
    let listener = TcpListener::bind("0.0.0.0:5000").await?;
    info!("🎥 Servidor de Vídeo (TCP) rodando na porta 5000");

    loop {
        match listener.accept().await {
            Ok((mut socket, addr)) => {
                info!("🔗 Cliente conectado no canal de Vídeo: {}", addr);
                
                // When a client connects, we start FFmpeg
                let config = CaptureConfig::default();
                match capture::spawn_ffmpeg(&config) {
                    Ok((mut child, mut stdout)) => {
                        info!("🎬 FFmpeg iniciado, enviando bytes brutos H.264 para o socket...");
                        
                        // Pipe stdout directly to the TCP socket
                        match tokio::io::copy(&mut stdout, &mut socket).await {
                            Ok(bytes) => {
                                info!("⏹️  Conexão de vídeo encerrada. Bytes enviados: {}", bytes);
                            }
                            Err(e) => {
                                warn!("⚠️  Conexão de vídeo interrompida: {}", e);
                            }
                        }

                        // Kill ffmpeg when client disconnects
                        info!("🛑 Matando FFmpeg...");
                        let _ = child.kill().await;
                        let _ = child.wait().await;
                    }
                    Err(e) => {
                        error!("❌ Falha ao iniciar FFmpeg: {}", e);
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
                let input_tx = input_tx.clone();

                tokio::spawn(async move {
                    let mut reader = BufReader::new(socket);
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
                                        let _ = input_tx.send(cmd).await;
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
