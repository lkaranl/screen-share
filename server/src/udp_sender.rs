use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tracing::{debug, info};

pub struct UdpSender {
    socket: Arc<UdpSocket>,
}

impl UdpSender {
    pub async fn bind(port: u16) -> Result<Self> {
        let addr = format!("0.0.0.0:{}", port);
        let socket = UdpSocket::bind(&addr)
            .await
            .context(format!("Falha ao fazer bind UDP na porta {}", port))?;

        info!("📡 Servidor de Vídeo (UDP + FEC) escutando em {}", addr);
        Ok(Self {
            socket: Arc::new(socket),
        })
    }

    /// Envia uma lista de pacotes RTP (shards de dados + paridade) com burst pacing
    pub async fn send_frame_packets(
        &self,
        packets: &[Vec<u8>],
        target_addr: SocketAddr,
    ) -> Result<()> {
        let batch_size = 32; // ~44 KB por rajada (Pacing estilo Sunshine)

        for (i, packet) in packets.iter().enumerate() {
            if let Err(e) = self.socket.send_to(packet, target_addr).await {
                debug!("⚠️ Erro ao enviar pacote UDP para {}: {}", target_addr, e);
            }

            // Pacing: a cada batch_size pacotes, cede a CPU para permitir que o kernel esvazie o buffer de envio
            if (i + 1) % batch_size == 0 && i + 1 < packets.len() {
                tokio::task::yield_now().await;
            }
        }

        Ok(())
    }
}
