use anyhow::{Context, Result};
use common::command::{ControlResponse, InputCommand};
use std::net::SocketAddr;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

pub struct ControlClient {
    tx: mpsc::Sender<InputCommand>,
}

impl ControlClient {
    pub async fn connect(server_addr: SocketAddr) -> Result<Self> {
        info!("🎮 Conectando ao canal de Controle do servidor em {}...", server_addr);
        let stream = TcpStream::connect(server_addr)
            .await
            .context(format!("Falha ao conectar no canal de controle TCP em {}", server_addr))?;

        let _ = stream.set_nodelay(true);
        let (read_half, mut write_half) = stream.into_split();

        let (tx, mut rx) = mpsc::channel::<InputCommand>(1024);

        // Task de envio de comandos (mouse, teclado, clipboard)
        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                if let Ok(mut json) = serde_json::to_string(&cmd) {
                    json.push('\n');
                    if let Err(e) = write_half.write_all(json.as_bytes()).await {
                        warn!("⚠️ Erro ao enviar comando de controle: {}", e);
                        break;
                    }
                }
            }
            debug!("🛑 Canal de envio de controle encerrado.");
        });

        // Task de leitura de respostas (Pong, ClipboardSync)
        tokio::spawn(async move {
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        info!("⏹️ Servidor de controle encerrou a conexão.");
                        break;
                    }
                    Ok(_) => {
                        if let Ok(resp) = serde_json::from_str::<ControlResponse>(&line) {
                            match resp {
                                ControlResponse::Pong { timestamp } => {
                                    let now = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis() as u64;
                                    let rtt = now.saturating_sub(timestamp);
                                    debug!("🏓 RTT de controle: {} ms", rtt);
                                }
                                ControlResponse::ClipboardSync { text } => {
                                    info!("📋 Clipboard recebido do servidor ({} bytes)", text.len());
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("❌ Erro na leitura de respostas do controle: {}", e);
                        break;
                    }
                }
            }
        });

        info!("✅ Conectado ao canal de Controle (TCP 5001)");
        Ok(Self { tx })
    }

    pub fn send(&self, cmd: InputCommand) {
        let _ = self.tx.try_send(cmd);
    }
}
