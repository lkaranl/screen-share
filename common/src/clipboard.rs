use anyhow::{anyhow, Result};
use std::io::Write;
use std::path::Path;
use std::process::Command;

/// Obtém o UID do usuário corrente ou logado
fn get_user_uid(username: &str) -> String {
    if let Ok(uid_str) = std::env::var("UID") {
        return uid_str;
    }
    if let Ok(output) = Command::new("id").arg("-u").arg(username).output() {
        if output.status.success() {
            if let Ok(uid_str) = String::from_utf8(output.stdout) {
                return uid_str.trim().to_string();
            }
        }
    }
    if let Ok(output) = Command::new("id").arg("-u").output() {
        if output.status.success() {
            if let Ok(uid_str) = String::from_utf8(output.stdout) {
                return uid_str.trim().to_string();
            }
        }
    }
    "1000".to_string()
}

/// Define o conteúdo da área de transferência do sistema (Wayland ou X11)
pub fn set_system_clipboard(text: &str) -> Result<()> {
    let username = std::env::var("SUDO_USER")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "user".to_string());
    let uid = get_user_uid(&username);

    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", uid));

    let wayland_display = std::env::var("WAYLAND_DISPLAY")
        .or_else(|_| {
            let p0 = format!("{}/wayland-0", runtime_dir);
            if Path::new(&p0).exists() {
                Ok("wayland-0".to_string())
            } else {
                Err(std::env::VarError::NotPresent)
            }
        })
        .unwrap_or_else(|_| "wayland-0".to_string());

    let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());
    let home_dir = if username == "root" {
        "/root".to_string()
    } else {
        format!("/home/{}", username)
    };
    let xauthority = std::env::var("XAUTHORITY")
        .unwrap_or_else(|_| format!("{}/.Xauthority", home_dir));

    // 1. Wayland via wl-copy (tenta no PATH, em /app/bin e em /usr/bin)
    for bin in &["wl-copy", "/app/bin/wl-copy", "/usr/bin/wl-copy", "/usr/local/bin/wl-copy"] {
        if let Ok(mut child) = Command::new(bin)
            .env("XDG_RUNTIME_DIR", &runtime_dir)
            .env("WAYLAND_DISPLAY", &wayland_display)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if let Ok(status) = child.wait() {
                if status.success() {
                    return Ok(());
                }
            }
        }
    }

    // 2. X11 via xclip
    for bin in &["xclip", "/app/bin/xclip", "/usr/bin/xclip"] {
        if let Ok(mut child) = Command::new(bin)
            .args(&["-selection", "clipboard"])
            .env("DISPLAY", &display)
            .env("XAUTHORITY", &xauthority)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if let Ok(status) = child.wait() {
                if status.success() {
                    return Ok(());
                }
            }
        }
    }

    // 3. X11 via xsel
    for bin in &["xsel", "/app/bin/xsel", "/usr/bin/xsel"] {
        if let Ok(mut child) = Command::new(bin)
            .args(&["-ib"])
            .env("DISPLAY", &display)
            .env("XAUTHORITY", &xauthority)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if let Ok(status) = child.wait() {
                if status.success() {
                    return Ok(());
                }
            }
        }
    }

    // 4. Se estiver em Flatpak, tenta rodar no host via flatpak-spawn
    if Path::new("/.flatpak-info").exists() {
        if let Ok(mut child) = Command::new("flatpak-spawn")
            .args(&["--host", "wl-copy"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if let Ok(status) = child.wait() {
                if status.success() {
                    return Ok(());
                }
            }
        }
    }

    Err(anyhow!("Nenhum utilitário de clipboard funcional encontrado (wl-copy/xclip/xsel)"))
}

/// Lê o conteúdo da área de transferência do sistema (Wayland ou X11)
pub fn get_system_clipboard() -> Result<String> {
    let username = std::env::var("SUDO_USER")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "user".to_string());
    let uid = get_user_uid(&username);

    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", uid));

    let wayland_display = std::env::var("WAYLAND_DISPLAY")
        .or_else(|_| {
            let p0 = format!("{}/wayland-0", runtime_dir);
            if Path::new(&p0).exists() {
                Ok("wayland-0".to_string())
            } else {
                Err(std::env::VarError::NotPresent)
            }
        })
        .unwrap_or_else(|_| "wayland-0".to_string());

    let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());
    let home_dir = if username == "root" {
        "/root".to_string()
    } else {
        format!("/home/{}", username)
    };
    let xauthority = std::env::var("XAUTHORITY")
        .unwrap_or_else(|_| format!("{}/.Xauthority", home_dir));

    // 1. Wayland via wl-paste
    for bin in &["wl-paste", "/app/bin/wl-paste", "/usr/bin/wl-paste", "/usr/local/bin/wl-paste"] {
        if let Ok(output) = Command::new(bin)
            .args(&["-n", "--no-newline"])
            .env("XDG_RUNTIME_DIR", &runtime_dir)
            .env("WAYLAND_DISPLAY", &wayland_display)
            .output()
        {
            if output.status.success() {
                if let Ok(text) = String::from_utf8(output.stdout) {
                    return Ok(text);
                }
            }
        }
    }

    // 2. X11 via xclip
    for bin in &["xclip", "/app/bin/xclip", "/usr/bin/xclip"] {
        if let Ok(output) = Command::new(bin)
            .args(&["-selection", "clipboard", "-o"])
            .env("DISPLAY", &display)
            .env("XAUTHORITY", &xauthority)
            .output()
        {
            if output.status.success() {
                if let Ok(text) = String::from_utf8(output.stdout) {
                    return Ok(text);
                }
            }
        }
    }

    // 3. X11 via xsel
    for bin in &["xsel", "/app/bin/xsel", "/usr/bin/xsel"] {
        if let Ok(output) = Command::new(bin)
            .args(&["-ob"])
            .env("DISPLAY", &display)
            .env("XAUTHORITY", &xauthority)
            .output()
        {
            if output.status.success() {
                if let Ok(text) = String::from_utf8(output.stdout) {
                    return Ok(text);
                }
            }
        }
    }

    // 4. Se estiver em Flatpak, tenta rodar no host via flatpak-spawn
    if Path::new("/.flatpak-info").exists() {
        if let Ok(output) = Command::new("flatpak-spawn")
            .args(&["--host", "wl-paste", "-n", "--no-newline"])
            .output()
        {
            if output.status.success() {
                if let Ok(text) = String::from_utf8(output.stdout) {
                    return Ok(text);
                }
            }
        }
    }

    Err(anyhow!("Não foi possível obter o conteúdo do clipboard"))
}
