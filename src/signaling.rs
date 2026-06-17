use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use webrtc::{
    ice_transport::ice_candidate::RTCIceCandidateInit,
    peer_connection::sdp::session_description::RTCSessionDescription,
};

use crate::{webrtc_session::create_session, AppState};

/// Handler Axum: faz upgrade da conexão HTTP para WebSocket.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Gerencia uma sessão WebSocket completa (sinalização WebRTC).
///
/// ## Protocolo de sinalização (JSON via WebSocket):
///
/// Browser → Servidor:
/// - `{"type":"offer","sdp":"..."}` — SDP offer do browser
/// - `{"type":"ice_candidate","candidate":"...","sdpMid":"...","sdpMLineIndex":0}`
///
/// Servidor → Browser:
/// - `{"type":"answer","sdp":"..."}` — SDP answer do servidor
/// - `{"type":"ice_candidate","candidate":"...","sdpMid":"...","sdpMLineIndex":0}`
async fn handle_socket(socket: WebSocket, state: AppState) {
    info!("🔌 Nova conexão WebSocket — iniciando sessão WebRTC");

    // Divide o WebSocket em sender (para browser) e receiver (do browser)
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Canal interno para enviar mensagens de saída de forma assíncrona
    // (ICE candidates chegam via callbacks do webrtc-rs, fora do loop principal)
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<String>();

    // Task separada para enviar mensagens ao browser (ICE candidates, etc.)
    let send_task = tokio::spawn(async move {
        while let Some(msg) = outbound_rx.recv().await {
            if let Err(e) = ws_tx.send(Message::Text(msg)).await {
                warn!("Erro enviando mensagem WebSocket: {}", e);
                break;
            }
        }
    });

    // Cria a sessão WebRTC (PeerConnection + track + DataChannel handler + FFmpeg)
    let pc = match create_session(state.input_tx.clone(), outbound_tx.clone()).await {
        Ok(pc) => pc,
        Err(e) => {
            error!("Falha ao criar sessão WebRTC: {}", e);
            return;
        }
    };

    // ── Loop de mensagens do browser ─────────────────────────────────────
    while let Some(result) = ws_rx.next().await {
        let msg = match result {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) => {
                info!("Browser fechou o WebSocket");
                break;
            }
            Ok(_) => continue, // ignora mensagens binárias/ping/pong
            Err(e) => {
                warn!("Erro no WebSocket: {}", e);
                break;
            }
        };

        let json: serde_json::Value = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(e) => {
                warn!("Mensagem inválida recebida: {} — {}", msg, e);
                continue;
            }
        };

        match json["type"].as_str() {
            // ── Offer do browser ─────────────────────────────────────────
            Some("offer") => {
                let sdp = match json["sdp"].as_str() {
                    Some(s) => s.to_owned(),
                    None => {
                        warn!("Offer sem campo 'sdp'");
                        continue;
                    }
                };

                info!("📨 Offer SDP recebido do browser");

                // Define o SDP remoto (offer do browser)
                let offer = match RTCSessionDescription::offer(sdp) {
                    Ok(o) => o,
                    Err(e) => {
                        error!("SDP offer inválido: {}", e);
                        continue;
                    }
                };

                if let Err(e) = pc.set_remote_description(offer).await {
                    error!("set_remote_description falhou: {}", e);
                    break;
                }

                // Cria o Answer (servidor aceita e responde com sua configuração)
                let answer = match pc.create_answer(None).await {
                    Ok(a) => a,
                    Err(e) => {
                        error!("create_answer falhou: {}", e);
                        break;
                    }
                };

                // Define o SDP local (nosso answer)
                if let Err(e) = pc.set_local_description(answer).await {
                    error!("set_local_description falhou: {}", e);
                    break;
                }

                // Recupera o SDP local completo e envia ao browser
                if let Some(local_desc) = pc.local_description().await {
                    let response = serde_json::json!({
                        "type": "answer",
                        "sdp": local_desc.sdp,
                    })
                    .to_string();

                    info!("📤 Enviando Answer SDP ao browser");
                    let _ = outbound_tx.send(response);
                }
            }

            // ── ICE candidate do browser ─────────────────────────────────
            Some("ice_candidate") => {
                let candidate = json["candidate"].as_str().unwrap_or("").to_owned();

                if candidate.is_empty() {
                    // ICE gathering completo (candidate vazio = fim)
                    continue;
                }

                let sdp_mid = json["sdpMid"].as_str().map(|s| s.to_owned());
                let sdp_mline_index = json["sdpMLineIndex"].as_u64().map(|n| n as u16);

                let init = RTCIceCandidateInit {
                    candidate,
                    sdp_mid,
                    sdp_mline_index,
                    username_fragment: None,
                };

                if let Err(e) = pc.add_ice_candidate(init).await {
                    warn!("add_ice_candidate falhou: {}", e);
                }
            }

            Some(t) => warn!("Tipo de mensagem desconhecido: '{}'", t),
            None => warn!("Mensagem sem campo 'type'"),
        }
    }

    // ── Limpeza ──────────────────────────────────────────────────────────
    info!("🔌 Sessão encerrada — fechando PeerConnection");
    pc.close().await.ok();
    send_task.abort();
}
