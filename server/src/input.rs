use anyhow::Result;
use evdev::{
    uinput::VirtualDeviceBuilder,
    AttributeSet, EventType, InputEvent, Key, RelativeAxisType,
};
use tokio::sync::mpsc;
use tracing::{error, info};

use serde::{Serialize, Deserialize};

/// Comandos de input enviados do WebRTC DataChannel para o thread uinput.
#[derive(Debug, Serialize, Deserialize)]
pub enum InputCommand {
    /// Movimento relativo do mouse (delta x, delta y)
    MouseMove { dx: i32, dy: i32 },
    /// Botão do mouse (0=esquerdo, 1=meio, 2=direito)
    MouseButton { button: u8, pressed: bool },
    /// Scroll do mouse (positivo = para baixo)
    MouseScroll { dy: i32 },
    /// Tecla do teclado (keycode Linux)
    Key { code: u16, pressed: bool },
}

/// Tipo público para o sender de input (Send + Sync para uso em callbacks WebRTC).
pub type InputSender = mpsc::Sender<InputCommand>;

/// Inicia o handler de input em uma thread OS dedicada.
///
/// Retorna um `Sender` que pode ser clonado e enviado para as tasks async.
/// A thread OS usa `blocking_recv()` para processar eventos sem bloquear o runtime tokio.
pub fn start_input_handler() -> Result<InputSender> {
    let (tx, rx) = mpsc::channel::<InputCommand>(256);

    std::thread::Builder::new()
        .name("input-handler".to_string())
        .spawn(move || {
            if let Err(e) = run_input_handler(rx) {
                error!("Input handler encerrou com erro: {}", e);
            }
        })?;

    Ok(tx)
}

/// Loop principal do handler de input — executa em thread OS (não async).
fn run_input_handler(mut rx: mpsc::Receiver<InputCommand>) -> Result<()> {
    // ── Configura dispositivo virtual de mouse ──────────────────────────
    let mut mouse_keys = AttributeSet::<Key>::new();
    mouse_keys.insert(Key::BTN_LEFT);
    mouse_keys.insert(Key::BTN_RIGHT);
    mouse_keys.insert(Key::BTN_MIDDLE);
    mouse_keys.insert(Key::BTN_SIDE);
    mouse_keys.insert(Key::BTN_EXTRA);

    let mut mouse_axes = AttributeSet::<RelativeAxisType>::new();
    mouse_axes.insert(RelativeAxisType::REL_X);
    mouse_axes.insert(RelativeAxisType::REL_Y);
    mouse_axes.insert(RelativeAxisType::REL_WHEEL);
    mouse_axes.insert(RelativeAxisType::REL_HWHEEL);

    let mut mouse = VirtualDeviceBuilder::new()?
        .name("screen-share-virtual-mouse")
        .with_keys(&mouse_keys)?
        .with_relative_axes(&mouse_axes)?
        .build()?;

    info!("🖱️  Mouse virtual criado");

    // ── Configura dispositivo virtual de teclado ────────────────────────
    let mut kb_keys = AttributeSet::<Key>::new();
    // Adiciona todos os keycodes Linux relevantes (0-255)
    for code in 1u16..=255 {
        // Key(code) — o construtor por valor é aceito pelo AttributeSet
        let _ = kb_keys.insert(Key::new(code));
    }

    let mut keyboard = VirtualDeviceBuilder::new()?
        .name("screen-share-virtual-keyboard")
        .with_keys(&kb_keys)?
        .build()?;

    info!("⌨️  Teclado virtual criado");

    // ── Loop de eventos ─────────────────────────────────────────────────
    while let Some(cmd) = rx.blocking_recv() {
        match cmd {
            InputCommand::MouseMove { dx, dy } => {
                let _ = mouse.emit(&[
                    InputEvent::new(EventType::RELATIVE, RelativeAxisType::REL_X.0, dx),
                    InputEvent::new(EventType::RELATIVE, RelativeAxisType::REL_Y.0, dy),
                    InputEvent::new(EventType::SYNCHRONIZATION, 0, 0),
                ]);
            }

            InputCommand::MouseButton { button, pressed } => {
                let btn_code = match button {
                    0 => Key::BTN_LEFT.code(),
                    1 => Key::BTN_MIDDLE.code(),
                    2 => Key::BTN_RIGHT.code(),
                    3 => Key::BTN_SIDE.code(),
                    4 => Key::BTN_EXTRA.code(),
                    _ => continue,
                };
                let _ = mouse.emit(&[
                    InputEvent::new(EventType::KEY, btn_code, if pressed { 1 } else { 0 }),
                    InputEvent::new(EventType::SYNCHRONIZATION, 0, 0),
                ]);
            }

            InputCommand::MouseScroll { dy } => {
                // REL_WHEEL: positivo = scroll up, negativo = scroll down
                let _ = mouse.emit(&[
                    InputEvent::new(EventType::RELATIVE, RelativeAxisType::REL_WHEEL.0, -dy),
                    InputEvent::new(EventType::SYNCHRONIZATION, 0, 0),
                ]);
            }

            InputCommand::Key { code, pressed } => {
                if code == 0 || code > 255 {
                    continue; // keycode inválido
                }
                let _ = keyboard.emit(&[
                    InputEvent::new(EventType::KEY, code, if pressed { 1 } else { 0 }),
                    InputEvent::new(EventType::SYNCHRONIZATION, 0, 0),
                ]);
            }
        }
    }

    info!("Input handler encerrado");
    Ok(())
}
