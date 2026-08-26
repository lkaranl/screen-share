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
        let addr: SocketAddr = format!("0.0.0.0:{}", port).parse()?;

        // Cria o socket com socket2 para aplicar configurações de nível de kernel
        let socket2_sock = socket2::Socket::new(
            if addr.is_ipv4() {
                socket2::Domain::IPV4
            } else {
                socket2::Domain::IPV6
            },
            socket2::Type::DGRAM,
            Some(socket2::Protocol::UDP),
        )?;

        // Buffers ampliados de 4MB para evitar perdas de pacotes do kernel
        let _ = socket2_sock.set_send_buffer_size(4 * 1024 * 1024);
        let _ = socket2_sock.set_recv_buffer_size(4 * 1024 * 1024);

        // QoS DSCP 40 (CS5 Video) estilo Sunshine para prioridade WMM 802.11e no roteador Wi-Fi
        let _ = socket2_sock.set_tos(40 << 2);
        let _ = socket2_sock.set_reuse_address(true);
        let _ = socket2_sock.set_nonblocking(true);

        socket2_sock.bind(&addr.into())?;

        let std_sock: std::net::UdpSocket = socket2_sock.into();
        let tokio_sock = UdpSocket::from_std(std_sock)
            .context(format!("Falha ao inicializar UdpSocket tokio na porta {}", port))?;

        info!("📡 Servidor de Vídeo (UDP + FEC + QoS DSCP) escutando em {}", addr);
        Ok(Self {
            socket: Arc::new(tokio_sock),
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
