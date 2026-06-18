use anyhow::Result;
use evdev::{
    uinput::VirtualDeviceBuilder, AbsInfo, AbsoluteAxisType, AttributeSet, EventType, InputEvent,
    Key, RelativeAxisType, UinputAbsSetup,
};
use tokio::sync::mpsc;
use tracing::{error, info};
use std::process::Command;
use std::io::Write;

use serde::{Serialize, Deserialize};

/// Comandos de input enviados do WebRTC DataChannel para o thread uinput.
#[derive(Debug, Serialize, Deserialize)]
pub enum InputCommand {
    /// Movimento absoluto do mouse (x, y normalizados de 0 a 32767)
    MouseMove { x: i32, y: i32 },
    /// Botão do mouse (0=esquerdo, 1=meio, 2=direito)
    MouseButton { button: u8, pressed: bool },
    /// Scroll do mouse (positivo = para baixo)
    MouseScroll { dy: i32 },
    /// Tecla do teclado (keycode Linux)
    Key { code: u16, pressed: bool },
    /// Sincronizar texto para colar
    ClipboardPaste { text: String },
    /// Requisitar texto copiado
    ClipboardRequest,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ControlResponse {
    ClipboardSync { text: String },
}

fn get_user_uid(username: &str) -> String {
    let output = Command::new("id")
        .args(&["-u", username])
        .output();
    if let Ok(out) = output {
        if out.status.success() {
            if let Ok(uid_str) = String::from_utf8(out.stdout) {
                return uid_str.trim().to_string();
            }
        }
    }
    "1000".to_string()
}

pub fn set_remote_clipboard(text: &str) -> Result<()> {
    let username = std::env::var("SUDO_USER")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "servidor".to_string());
    let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());
    let home_dir = if username == "root" {
        "/root".to_string()
    } else {
        format!("/home/{}", username)
    };
    let xauthority = format!("{}/.Xauthority", home_dir);
    let uid = get_user_uid(&username);

    // Tenta xclip diretamente (com DISPLAY e XAUTHORITY do usuario)
    let child = Command::new("xclip")
        .args(&["-selection", "clipboard"])
        .env("DISPLAY", &display)
        .env("XAUTHORITY", &xauthority)
        .stdin(std::process::Stdio::piped())
        .spawn();

    if let Ok(mut c) = child {
        if let Some(mut stdin) = c.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        if let Ok(status) = c.wait() {
            if status.success() {
                return Ok(());
            }
        }
    }

    // Tenta xsel diretamente
    let child = Command::new("xsel")
        .args(&["-ib"])
        .env("DISPLAY", &display)
        .env("XAUTHORITY", &xauthority)
        .stdin(std::process::Stdio::piped())
        .spawn();

    if let Ok(mut c) = child {
        if let Some(mut stdin) = c.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        if let Ok(status) = c.wait() {
            if status.success() {
                return Ok(());
            }
        }
    }

    // Tenta wl-copy (Wayland)
    let child = Command::new("wl-copy")
        .env("XDG_RUNTIME_DIR", format!("/run/user/{}", uid))
        .stdin(std::process::Stdio::piped())
        .spawn();

    if let Ok(mut c) = child {
        if let Some(mut stdin) = c.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        if let Ok(status) = c.wait() {
            if status.success() {
                return Ok(());
            }
        }
    }

    // Se falhar diretamente, tenta via sudo -u (fallback legado)
    let child = Command::new("sudo")
        .args(&["-u", &username, "env", &format!("DISPLAY={}", display), &format!("XAUTHORITY={}", xauthority), "xclip", "-selection", "clipboard"])
        .stdin(std::process::Stdio::piped())
        .spawn();

    if let Ok(mut c) = child {
        if let Some(mut stdin) = c.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        if let Ok(status) = c.wait() {
            if status.success() {
                return Ok(());
            }
        }
    }

    Err(anyhow::anyhow!("Nenhum utilitário de clipboard funcional encontrado"))
}

pub fn get_remote_clipboard() -> Result<String> {
    let username = std::env::var("SUDO_USER")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "servidor".to_string());
    let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());
    let home_dir = if username == "root" {
        "/root".to_string()
    } else {
        format!("/home/{}", username)
    };
    let xauthority = format!("{}/.Xauthority", home_dir);
    let uid = get_user_uid(&username);

    // Tenta xclip diretamente
    let output = Command::new("xclip")
        .args(&["-selection", "clipboard", "-o"])
        .env("DISPLAY", &display)
        .env("XAUTHORITY", &xauthority)
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            if let Ok(text) = String::from_utf8(out.stdout) {
                return Ok(text);
            }
        }
    }

    // Tenta xsel diretamente
    let output = Command::new("xsel")
        .args(&["-ob"])
        .env("DISPLAY", &display)
        .env("XAUTHORITY", &xauthority)
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            if let Ok(text) = String::from_utf8(out.stdout) {
                return Ok(text);
            }
        }
    }

    // Tenta wl-paste (Wayland)
    let output = Command::new("wl-paste")
        .args(&["-n"])
        .env("XDG_RUNTIME_DIR", format!("/run/user/{}", uid))
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            if let Ok(text) = String::from_utf8(out.stdout) {
                return Ok(text);
            }
        }
    }

    // Fallback legado com sudo -u
    let output = Command::new("sudo")
        .args(&["-u", &username, "env", &format!("DISPLAY={}", display), &format!("XAUTHORITY={}", xauthority), "xclip", "-selection", "clipboard", "-o"])
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            if let Ok(text) = String::from_utf8(out.stdout) {
                return Ok(text);
            }
        }
    }

    Err(anyhow::anyhow!("Não foi possível obter o clipboard"))
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

    // Eixos relativos para scroll
    let mut mouse_rel_axes = AttributeSet::<RelativeAxisType>::new();
    mouse_rel_axes.insert(RelativeAxisType::REL_WHEEL);
    mouse_rel_axes.insert(RelativeAxisType::REL_HWHEEL);

    // Eixos absolutos para X e Y (0 a 32767)
    let abs_x = UinputAbsSetup::new(
        AbsoluteAxisType::ABS_X,
        AbsInfo::new(0, 0, 32767, 0, 0, 0),
    );
    let abs_y = UinputAbsSetup::new(
        AbsoluteAxisType::ABS_Y,
        AbsInfo::new(0, 0, 32767, 0, 0, 0),
    );

    let mut mouse = VirtualDeviceBuilder::new()?
        .name("screen-share-virtual-mouse")
        .with_keys(&mouse_keys)?
        .with_relative_axes(&mouse_rel_axes)?
        .with_absolute_axis(&abs_x)?
        .with_absolute_axis(&abs_y)?
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
            InputCommand::MouseMove { x, y } => {
                let _ = mouse.emit(&[
                    InputEvent::new(EventType::ABSOLUTE, AbsoluteAxisType::ABS_X.0, x),
                    InputEvent::new(EventType::ABSOLUTE, AbsoluteAxisType::ABS_Y.0, y),
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
                    InputEvent::new(EventType::RELATIVE, RelativeAxisType::REL_WHEEL.0, dy),
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

            InputCommand::ClipboardPaste { .. } | InputCommand::ClipboardRequest => {}
        }
    }

    info!("Input handler encerrado");
    Ok(())
}
