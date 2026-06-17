use axum::{Router, routing::get};
use tokio::sync::mpsc;
use tracing::info;
use anyhow::Result;

mod capture;
mod input;
mod signaling;
mod webrtc_session;

use input::InputCommand;

/// Estado compartilhado da aplicação.
/// `input_tx` permite enviar comandos de input para o thread OS
/// que controla os dispositivos virtuais via uinput.
#[derive(Clone)]
pub struct AppState {
    pub input_tx: mpsc::Sender<InputCommand>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Inicializa logging estruturado
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "screen_share=info,webrtc=warn".to_string()),
        )
        .init();

    // Inicia o handler de input (thread OS com dispositivos uinput)
    let input_tx = input::start_input_handler()?;
    info!("✅ Dispositivos virtuais de input criados (mouse + teclado)");

    let state = AppState { input_tx };

    // Define as rotas do servidor
    let app = Router::new()
        .route("/", get(serve_index))
        .route("/ws", get(signaling::ws_handler))
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "39482".to_string());
    let addr = format!("0.0.0.0:{}", port);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let local_addr = listener.local_addr()?;
    let actual_port = local_addr.port();

    info!("🖥️  Screen Share server iniciando em http://0.0.0.0:{}", actual_port);
    info!("📱 Abra http://<IP-DO-SERVIDOR>:{} no browser", actual_port);
    info!("🔑 Precisa rodar como root para kmsgrab funcionar");

    axum::serve(listener, app).await?;

    Ok(())
}

/// Serve o HTML da UI diretamente do binário compilado (sem arquivos externos).
async fn serve_index() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("../static/index.html"))
}
