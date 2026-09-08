use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
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
mod gui;

use capture::CaptureConfig;
use fec::FecEncoder;
use gui::{ServerControlPanel, ServerUiState};
use nal_extractor::NalExtractor;
use udp_sender::UdpSender;

fn detect_local_ip() -> String {
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("1.1.1.1:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                let ip = addr.ip().to_string();
                if ip != "0.0.0.0" {
                    return ip;
                }
            }
        }
    }
    std::process::Command::new("hostname")
        .arg("-I")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .and_then(|s| s.split_whitespace().next().map(|ip| ip.to_string()))
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "server=info".to_string()))
        .init();

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    let mut codec = capture::VideoCodec::HEVC; // Padrão HEVC de alta performance
    let mut is_headless = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--codec" => {
                if i + 1 < args.len() {
                    match args[i + 1].to_lowercase().as_str() {
                        "hevc" | "h265" => codec = capture::VideoCodec::HEVC,
                        "av1" => codec = capture::VideoCodec::AV1,
                        "h264" => codec = capture::VideoCodec::H264,
                        other => warn!("⚠️ Codec desconhecido '{}', usando padrão HEVC", other),
                    }
                    i += 1;
                }
            }
            "--headless" => {
                is_headless = true;
            }
            "--help" | "-h" => {
                println!("Screen Share Server (Linux)");
                println!("Uso: server [OPÇÕES]");
                println!();
                println!("Opções:");
                println!("  --codec <hevc|h264|av1>  Codec de vídeo padrão (padrão: hevc)");
                println!("  --headless               Executa exclusivamente no terminal sem abrir janela gráfica");
                println!("  --help, -h               Exibe esta mensagem de ajuda");
                return Ok(());
            }
            _ => {}
        }
        i += 1;
    }

    let local_ip = detect_local_ip();
    info!("📡 IP local detectado: {}", local_ip);

    // Inicializa dispositivos virtuais de input (/dev/uinput)
    let (input_tx, uinput_active) = match input::start_input_handler() {
        Ok(tx) => {
            info!("✅ Dispositivos virtuais de input criados (mouse + teclado)");
            (Some(tx), true)
        }
        Err(e) => {
            warn!("⚠️ /dev/uinput não acessível: {}. Controle remoto desabilitado.", e);
            (None, false)
        }
    };

    let active_clients = Arc::new(AtomicU32::new(0));
    let is_running = Arc::new(AtomicBool::new(true));
    let active_codec = Arc::new(AtomicU8::new(match codec {
        capture::VideoCodec::H264 => 0,
        capture::VideoCodec::HEVC => 1,
        capture::VideoCodec::AV1 => 2,
    }));

    let ui_state = ServerUiState {
        local_ip: Arc::new(Mutex::new(local_ip)),
        video_port: 5000,
        control_port: 5001,
        uinput_active,
        active_clients: active_clients.clone(),
        is_running: is_running.clone(),
        active_codec: active_codec.clone(),
    };

    // Inicializa runtime multithread do Tokio em segundo plano
    let rt = tokio::runtime::Runtime::new()?;
    let is_running_bg = is_running.clone();
    let is_running_ctrl = is_running.clone();
    let active_clients_bg = active_clients.clone();
    let active_codec_bg = active_codec.clone();

    rt.spawn(async move {
        // Spawn Input/Control TCP Server (Porta 5001)
        if let Some(tx) = input_tx {
            let tx_clone = tx.clone();
            tokio::spawn(async move {
                if let Err(e) = run_control_server(tx_clone, is_running_ctrl).await {
                    error!("Erro no servidor de controle: {}", e);
                }
            });
        }

        // Run Video UDP + FEC Server (Porta 5000)
        if let Err(e) = run_video_udp_server(codec, active_clients_bg, active_codec_bg, is_running_bg).await {
            error!("Erro no servidor de vídeo: {}", e);
        }
    });

    // Detecta se há servidor gráfico disponível
    let has_display = std::env::var("WAYLAND_DISPLAY").is_ok() || std::env::var("DISPLAY").is_ok();
    if !is_headless && has_display {
        info!("🖥️ Iniciando painel visual do servidor...");
        let panel = ServerControlPanel::new(ui_state);
        panel.run()?;
    } else {
        info!("🖥️ Executando em modo headless (terminal). Pressione Ctrl+C para encerrar.");
        while is_running.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }

    info!("🛑 Servidor encerrado com sucesso.");
    Ok(())
}

async fn run_video_udp_server(
    default_codec: capture::VideoCodec,
    active_clients: Arc<AtomicU32>,
    shared_codec: Arc<AtomicU8>,
    is_running: Arc<AtomicBool>,
) -> Result<()> {
    let udp_sender = Arc::new(UdpSender::bind(5000).await?);
    let fec_encoder = Arc::new(FecEncoder::new(20)); // 20% de tolerância a perdas (Reed-Solomon)

    // Handshake de sessão exclusivamente via TCP na porta 5000
    let tcp_listener = TcpListener::bind("0.0.0.0:5000").await?;
    info!("🎥 Servidor de Vídeo pronto | Codec padrão: {:?} | Aguardando cliente TCP na porta 5000", default_codec);

    // Canal de cancelamento da sessão ativa (apenas um FFmpeg por vez)
    let mut session_cancel: Option<tokio::sync::oneshot::Sender<()>> = None;

    while is_running.load(Ordering::Relaxed) {
        tokio::select! {
            accept_res = tcp_listener.accept() => {
                match accept_res {
                    Ok((mut socket, client_addr)) => {
                        info!("🔗 Handshake TCP de sessão de {}", client_addr);
                        active_clients.fetch_add(1, Ordering::SeqCst);
                        let _ = socket.set_nodelay(true);

                        // Lê dados do handshake (2 bytes porta + 1 byte codec opcional)
                        let mut handshake_buf = [0u8; 3];
                        let mut active_codec = default_codec;
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
                        shared_codec.store(codec_id, Ordering::Relaxed);

                let client_udp_target = SocketAddr::new(client_addr.ip(), client_video_port);
                info!("🚀 Iniciando transmissão UDP de vídeo ({:?}) para {} (porta {})", active_codec, client_udp_target, client_video_port);

                // Cancela a sessão anterior se existir
                if let Some(tx) = session_cancel.take() {
                    info!("🔄 Novo cliente — encerrando sessão anterior...");
                    let _ = tx.send(());
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }

                let mut config = CaptureConfig::default();
                config.codec = active_codec;

                match capture::spawn_ffmpeg(&config) {
                    Ok((mut child, mut stdout)) => {
                        info!("🎬 FFmpeg iniciado ({:?}), transmitindo via RTP/UDP + FEC...", active_codec);

                        let udp_sender_clone = udp_sender.clone();
                        let fec_encoder_clone = fec_encoder.clone();
                        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();
                        session_cancel = Some(cancel_tx);
                        let clients_counter = active_clients.clone();

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
                            clients_counter.fetch_sub(1, Ordering::SeqCst);
                        });
                    }
                    Err(e) => {
                        error!("❌ Falha ao iniciar FFmpeg: {:#}", e);
                        active_clients.fetch_sub(1, Ordering::SeqCst);
                    }
                }
                    }
                    Err(e) => {
                        error!("Erro ao aceitar conexão TCP de vídeo: {}", e);
                    }
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(200)) => {}
        }
    }

    Ok(())
}

async fn run_control_server(input_tx: input::InputSender, is_running: Arc<AtomicBool>) -> Result<()> {
    let tcp_listener = TcpListener::bind("0.0.0.0:5001").await?;
    info!("🎮 Servidor de Controle aguardando na porta TCP 5001");

    while is_running.load(Ordering::Relaxed) {
        tokio::select! {
            accept_res = tcp_listener.accept() => {
                match accept_res {
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
                                                        match input::get_remote_clipboard() {
                                                            Ok(text) => {
                                                                info!("📋 Enviando clipboard remoto ao cliente ({} bytes)", text.len());
                                                                let resp = input::ControlResponse::ClipboardSync { text };
                                                                if let Ok(mut resp_json) = serde_json::to_string(&resp) {
                                                                    resp_json.push('\n');
                                                                    let _ = write_half.write_all(resp_json.as_bytes()).await;
                                                                }
                                                            }
                                                            Err(e) => {
                                                                warn!("⚠️ Não foi possível obter o clipboard remoto: {}", e);
                                                            }
                                                        }
                                                    }
                                                    input::InputCommand::ClipboardPaste { text } => {
                                                        info!("📋 Recebido texto para colar no servidor ({} bytes)", text.len());
                                                        if let Err(e) = input::set_remote_clipboard(&text) {
                                                            warn!("⚠️ Falha ao setar clipboard no servidor: {}", e);
                                                        }
                                                        // Garante que Super (125/126) não interfira no Ctrl+V
                                                        let _ = input_tx.send(input::InputCommand::Key { code: 125, pressed: false }).await;
                                                        let _ = input_tx.send(input::InputCommand::Key { code: 126, pressed: false }).await;
                                                        tokio::time::sleep(std::time::Duration::from_millis(15)).await;

                                                        // Pulsa Ctrl+V limpo
                                                        let _ = input_tx.send(input::InputCommand::Key { code: 29, pressed: true }).await;
                                                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                                                        let _ = input_tx.send(input::InputCommand::Key { code: 47, pressed: true }).await;
                                                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                                                        let _ = input_tx.send(input::InputCommand::Key { code: 47, pressed: false }).await;
                                                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
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
            _ = tokio::time::sleep(std::time::Duration::from_millis(200)) => {}
        }
    }

    Ok(())
}
