use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InputCommand {
    /// Movimento absoluto do mouse (x, y normalizados de 0 a 32767)
    MouseMove { x: i32, y: i32 },
    /// Movimento relativo do mouse (dx, dy em pixels, ideal para jogos 3D)
    MouseMoveRelative { dx: i32, dy: i32 },
    /// Botão do mouse (0=esquerdo, 1=meio, 2=direito, 3=lateral, 4=extra)
    MouseButton { button: u8, pressed: bool },
    /// Scroll do mouse (positivo = para cima, negativo = para baixo)
    MouseScroll { dy: i32 },
    /// Tecla do teclado (keycode Linux evdev)
    Key { code: u16, pressed: bool },
    /// Sincronizar texto para colar
    ClipboardPaste { text: String },
    /// Requisitar texto copiado
    ClipboardRequest,
    /// Medição de latência RTT
    Ping { timestamp: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ControlResponse {
    ClipboardSync { text: String },
    Pong { timestamp: u64 },
}
