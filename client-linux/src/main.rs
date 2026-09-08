use anyhow::Result;
use std::net::SocketAddr;
use tokio::sync::mpsc;
use tracing::{info, warn};

mod control;
mod decoder;
mod display;
mod network;
mod scancode;

use control::ControlClient;
use decoder::VideoDecoder;
use display::DisplayWindow;
use network::NetworkReceiver;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "client=info".to_string()))
        .init();

    // Argumentos de linha de comando
    let args: Vec<String> = std::env::args().collect();
    let mut host = "127.0.0.1".to_string();
    let mut video_port: u16 = 5000;
    let mut control_port: u16 = 5001;
    let mut local_udp_port: u16 = 50000;
    let mut codec_id: u8 = 1; // Padrão HEVC (1), H.264 (0)
    let mut width: u32 = 1920;
    let mut height: u32 = 1080;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--host" | "-h" => {
                if i + 1 < args.len() {
                    host = args[i + 1].clone();
                    i += 1;
                }
            }
            "--port" | "-p" => {
                if i + 1 < args.len() {
                    if let Ok(p) = args[i + 1].parse() {
                        video_port = p;
                    }
                    i += 1;
                }
            }
            "--control-port" => {
                if i + 1 < args.len() {
                    if let Ok(p) = args[i + 1].parse() {
                        control_port = p;
                    }
                    i += 1;
                }
            }
            "--udp-port" => {
                if i + 1 < args.len() {
                    if let Ok(p) = args[i + 1].parse() {
                        local_udp_port = p;
                    }
                    i += 1;
                }
            }
            "--codec" => {
                if i + 1 < args.len() {
                    match args[i + 1].to_lowercase().as_str() {
                        "h264" | "avc" => codec_id = 0,
                        "hevc" | "h265" => codec_id = 1,
                        other => warn!("⚠️ Codec desconhecido '{}', usando HEVC", other),
                    }
                    i += 1;
                }
            }
            "--width" => {
                if i + 1 < args.len() {
                    if let Ok(w) = args[i + 1].parse() {
                        width = w;
                    }
                    i += 1;
                }
            }
            "--height" => {
                if i + 1 < args.len() {
                    if let Ok(h) = args[i + 1].parse() {
                        height = h;
                    }
                    i += 1;
                }
            }
            "--help" => {
                println!("Screen Share Client (Linux)");
                println!("Uso: client-linux [OPÇÕES]");
                println!();
                println!("Opções:");
                println!("  --host, -h <IP>         IP do servidor remoto (padrão: 127.0.0.1)");
                println!("  --port, -p <PORTA>      Porta de vídeo TCP/UDP do servidor (padrão: 5000)");
                println!("  --control-port <PORTA>  Porta de controle TCP do servidor (padrão: 5001)");
                println!("  --udp-port <PORTA>      Porta local para receber stream UDP (padrão: 50000)");
                println!("  --codec <h264|hevc>     Codec de vídeo solicitado (padrão: hevc)");
                println!("  --width <LARGURA>       Largura da exibição em pixels (padrão: 1920)");
                println!("  --height <ALTURA>       Altura da exibição em pixels (padrão: 1080)");
                return Ok(());
            }
            _ => {
                // Permite passar apenas o IP diretamente: client-linux 192.168.1.50
                if !args[i].starts_with('-') {
                    host = args[i].clone();
                }
            }
        }
        i += 1;
    }

    info!("🚀 Iniciando Screen Share Client para {}:{}", host, video_port);

    // Inicializa o runtime multithread do Tokio para as tarefas de rede e decodificação
    let rt = tokio::runtime::Runtime::new()?;

    let (video_decoder, control_client, _network_receiver) = rt.block_on(async {
        // 1. Conecta ao canal de controle TCP 5001
        let control_addr: SocketAddr = format!("{}:{}", host, control_port).parse()?;
        let control_client = ControlClient::connect(control_addr).await?;

        // 2. Canal de frames atômicos entre NetworkReceiver e VideoDecoder
        let (frame_tx, frame_rx) = mpsc::channel(64);

        // 3. Inicia o decodificador de vídeo (FFmpeg subprocess em background)
        let video_decoder = VideoDecoder::start(codec_id, width as usize, height as usize, frame_rx)?;

        // 4. Inicia a recepção de rede (Handshake TCP 5000 + UDP listener com FEC)
        let network_receiver = NetworkReceiver::start(&host, video_port, local_udp_port, codec_id, frame_tx).await?;

        Ok::<(VideoDecoder, ControlClient, NetworkReceiver), anyhow::Error>((video_decoder, control_client, network_receiver))
    })?;

    // 5. Inicia a janela gráfica de exibição SDL2 diretamente na thread principal (requisito Wayland/X11/macOS)
    let display = DisplayWindow::new(width, height)?;
    display.run(video_decoder, control_client)?;

    info!("🛑 Client encerrado com sucesso.");
    Ok(())
}
