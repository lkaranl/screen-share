use anyhow::{Context, Result};
use common::{DecodedFrame, FecDecoder, RTP_PACKET_MAX_SIZE};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

pub struct NetworkReceiver {
    _tcp_session: TcpStream,
}

impl NetworkReceiver {
    pub async fn start(
        server_host: &str,
        server_port: u16,
        local_udp_port: u16,
        codec_id: u8,
        frame_tx: mpsc::Sender<DecodedFrame>,
    ) -> Result<Self> {
        // 1. Handshake TCP de sessão na porta 5000
        let server_tcp_addr = format!("{}:{}", server_host, server_port);
        info!("🔗 Conectando ao servidor para Handshake de Vídeo em {}...", server_tcp_addr);

        let mut tcp_stream = TcpStream::connect(&server_tcp_addr)
            .await
            .context(format!("Falha ao conectar ao servidor de vídeo em {}", server_tcp_addr))?;

        let _ = tcp_stream.set_nodelay(true);

        // Envia os 3 bytes de handshake: [port_high, port_low, codec_id]
        let handshake_bytes = [
            (local_udp_port >> 8) as u8,
            (local_udp_port & 0xFF) as u8,
            codec_id,
        ];
        tcp_stream.write_all(&handshake_bytes).await?;
        info!(
            "🤝 Handshake de vídeo enviado | Porta UDP local: {} | Codec: {}",
            local_udp_port,
            if codec_id == 1 { "HEVC" } else { "H.264" }
        );

        // 2. Socket UDP para recepção de pacotes RTP/FEC
        let bind_addr: SocketAddr = format!("0.0.0.0:{}", local_udp_port).parse()?;
        let socket2_sock = socket2::Socket::new(
            socket2::Domain::IPV4,
            socket2::Type::DGRAM,
            Some(socket2::Protocol::UDP),
        )?;

        let _ = socket2_sock.set_recv_buffer_size(4 * 1024 * 1024); // 4MB buffer
        let _ = socket2_sock.set_reuse_address(true);
        let _ = socket2_sock.set_nonblocking(true);
        socket2_sock.bind(&bind_addr.into())?;

        let std_sock: std::net::UdpSocket = socket2_sock.into();
        let tokio_udp = Arc::new(UdpSocket::from_std(std_sock)?);
        info!("📡 Receptor UDP escutando em {}", bind_addr);

        // 3. Task de recepção e reconstrução Reed-Solomon FEC
        let udp_clone = tokio_udp.clone();
        tokio::spawn(async move {
            let mut fec_decoder = FecDecoder::new(16);
            let mut buf = vec![0u8; RTP_PACKET_MAX_SIZE + 256];

            loop {
                match udp_clone.recv_from(&mut buf).await {
                    Ok((len, _from_addr)) => {
                        let packet = &buf[..len];
                        match fec_decoder.process_packet(packet) {
                            Ok(Some(decoded_frame)) => {
                                if let Err(_) = frame_tx.send(decoded_frame).await {
                                    info!("🛑 Canal de entrega de frames encerrado.");
                                    break;
                                }
                            }
                            Ok(None) => {}
                            Err(e) => {
                                warn!("⚠️ Erro no processamento FEC: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("❌ Erro na recepção do pacote UDP: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(Self {
            _tcp_session: tcp_stream,
        })
    }
}
