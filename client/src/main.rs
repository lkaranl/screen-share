use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use sdl2::event::Event;
use sdl2::keyboard::Scancode;
use sdl2::pixels::PixelFormatEnum;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::{BufRead, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Serialize, Deserialize)]
pub enum InputCommand {
    MouseMove { x: i32, y: i32 },
    MouseButton { button: u8, pressed: bool },
    MouseScroll { dy: i32 },
    Key { code: u16, pressed: bool },
    ClipboardPaste { text: String },
    ClipboardRequest,
    Ping { timestamp: u64 },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ControlResponse {
    ClipboardSync { text: String },
    Pong { timestamp: u64 },
}

// Map SDL2 Scancode to Linux Input Event Keycode
fn map_scancode_to_linux(scancode: Scancode) -> u16 {
    match scancode {
        Scancode::A => 30,
        Scancode::B => 48,
        Scancode::C => 46,
        Scancode::D => 32,
        Scancode::E => 18,
        Scancode::F => 33,
        Scancode::G => 34,
        Scancode::H => 35,
        Scancode::I => 23,
        Scancode::J => 36,
        Scancode::K => 37,
        Scancode::L => 38,
        Scancode::M => 50,
        Scancode::N => 49,
        Scancode::O => 24,
        Scancode::P => 25,
        Scancode::Q => 16,
        Scancode::R => 19,
        Scancode::S => 31,
        Scancode::T => 20,
        Scancode::U => 22,
        Scancode::V => 47,
        Scancode::W => 17,
        Scancode::X => 45,
        Scancode::Y => 21,
        Scancode::Z => 44,
        Scancode::Num1 => 2,
        Scancode::Num2 => 3,
        Scancode::Num3 => 4,
        Scancode::Num4 => 5,
        Scancode::Num5 => 6,
        Scancode::Num6 => 7,
        Scancode::Num7 => 8,
        Scancode::Num8 => 9,
        Scancode::Num9 => 10,
        Scancode::Num0 => 11,
        Scancode::Return => 28,
        Scancode::Escape => 1,
        Scancode::Backspace => 14,
        Scancode::Tab => 15,
        Scancode::Space => 57,

        // Modificadores
        Scancode::LShift => 42,
        Scancode::RShift => 54,
        Scancode::LCtrl => 29,
        Scancode::RCtrl => 97,
        Scancode::LAlt => 56,
        Scancode::RAlt => 100,
        Scancode::LGui => 125,
        Scancode::RGui => 126,

        // Símbolos e Caracteres Especiais
        Scancode::Minus => 12,
        Scancode::Equals => 13,
        Scancode::LeftBracket => 26,
        Scancode::RightBracket => 27,
        Scancode::Semicolon => 39,
        Scancode::Apostrophe => 40,
        Scancode::Grave => 41,
        Scancode::Backslash => 43,
        Scancode::Comma => 51,
        Scancode::Period => 52,
        Scancode::Slash => 53,

        // Setas direcionais
        Scancode::Up => 103,
        Scancode::Down => 108,
        Scancode::Left => 105,
        Scancode::Right => 106,

        // Teclas do Sistema e Navegação
        Scancode::Insert => 110,
        Scancode::Delete => 111,
        Scancode::Home => 102,
        Scancode::End => 107,
        Scancode::PageUp => 104,
        Scancode::PageDown => 109,
        Scancode::CapsLock => 58,
        Scancode::NumLockClear => 69,
        Scancode::ScrollLock => 70,

        // Teclas de Função
        Scancode::F1 => 59,
        Scancode::F2 => 60,
        Scancode::F3 => 61,
        Scancode::F4 => 62,
        Scancode::F5 => 63,
        Scancode::F6 => 64,
        Scancode::F7 => 65,
        Scancode::F8 => 66,
        Scancode::F9 => 67,
        Scancode::F10 => 68,
        Scancode::F11 => 87,
        Scancode::F12 => 88,

        _ => 0,
    }
}

struct FrameData {
    width: u32,
    height: u32,
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
    y_pitch: usize,
    u_pitch: usize,
    v_pitch: usize,
}

#[derive(Serialize, Deserialize, Default)]
struct LauncherConfig {
    last_ip: String,
    history: Vec<String>,
}

impl LauncherConfig {
    fn get_path() -> Option<PathBuf> {
        let home = std::env::var("HOME").ok()?;
        let mut path = PathBuf::from(home);
        path.push(".config");
        path.push("rs-view");
        path.push("config.json");
        Some(path)
    }

    fn load() -> Self {
        if let Some(path) = Self::get_path() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(config) = serde_json::from_str::<LauncherConfig>(&content) {
                    return config;
                }
            }
        }
        Self::default()
    }

    fn save(&self) {
        if let Some(path) = Self::get_path() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(content) = serde_json::to_string_pretty(self) {
                let _ = fs::write(&path, content);
            }
        }
    }
}

struct LauncherApp {
    ip: String,
    history: Vec<String>,
    selected_ip: Arc<Mutex<Option<String>>>,
}

impl LauncherApp {
    fn new(config: &LauncherConfig, selected_ip: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            ip: config.last_ip.clone(),
            history: config.history.clone(),
            selected_ip,
        }
    }
}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        eframe::egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(15.0);
                ui.heading(
                    eframe::egui::RichText::new("RS-View")
                        .size(32.0)
                        .strong()
                        .color(eframe::egui::Color32::from_rgb(0, 191, 255))
                );
                ui.label(
                    eframe::egui::RichText::new("Compartilhamento de Tela Ultra Latência")
                        .size(12.0)
                        .italics()
                );
                ui.add_space(20.0);
            });

            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.vertical(|ui| {
                    ui.label(eframe::egui::RichText::new("Endereço IP do Host:").strong());
                    ui.add_space(5.0);
                    let text_edit = ui.add(
                        eframe::egui::TextEdit::singleline(&mut self.ip)
                            .hint_text("ex: 192.168.1.50")
                            .desired_width(f32::INFINITY)
                    );
                    
                    if text_edit.lost_focus() && ctx.input(|i| i.key_pressed(eframe::egui::Key::Enter)) {
                        let ip = self.ip.trim().to_string();
                        if !ip.is_empty() {
                            if let Ok(mut lock) = self.selected_ip.lock() {
                                *lock = Some(ip);
                            }
                            ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                        }
                    }
                });
            });

            ui.add_space(15.0);

            ui.vertical_centered(|ui| {
                let connect_btn = eframe::egui::Button::new(
                    eframe::egui::RichText::new("Conectar")
                        .size(16.0)
                        .strong()
                        .color(eframe::egui::Color32::WHITE)
                )
                .fill(eframe::egui::Color32::from_rgb(0, 122, 255))
                .min_size(eframe::egui::vec2(120.0, 35.0));

                if ui.add(connect_btn).clicked() {
                    let ip = self.ip.trim().to_string();
                    if !ip.is_empty() {
                        if let Ok(mut lock) = self.selected_ip.lock() {
                            *lock = Some(ip);
                        }
                        ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                    }
                }
            });

            if !self.history.is_empty() {
                ui.add_space(20.0);
                ui.separator();
                ui.add_space(10.0);
                ui.label(eframe::egui::RichText::new("Últimos Conectados:").strong());
                ui.add_space(5.0);

                eframe::egui::ScrollArea::vertical().max_height(80.0).show(ui, |ui| {
                    for prev_ip in &self.history {
                        ui.horizontal(|ui| {
                            let btn = eframe::egui::Button::new(
                                eframe::egui::RichText::new(prev_ip)
                                    .color(eframe::egui::Color32::from_rgb(180, 180, 180))
                            )
                            .frame(false);
                            
                            if ui.add(btn).clicked() {
                                self.ip = prev_ip.clone();
                            }
                        });
                    }
                });
            }
        });
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    
    let (server_ip, codec_hint) = if args.len() >= 2 {
        let ip = args[1].clone();
        let mut codec = None;
        if let Some(pos) = args.iter().position(|x| x == "--codec") {
            if pos + 1 < args.len() {
                let codec_str = args[pos + 1].to_lowercase();
                if codec_str == "hevc" || codec_str == "h265" {
                    codec = Some("hevc".to_string());
                } else if codec_str == "h264" {
                    codec = Some("h264".to_string());
                }
            }
        }
        (Some(ip), codec)
    } else {
        let config = LauncherConfig::load();
        let options = eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_title("RS-View - Conexão")
                .with_inner_size([350.0, 320.0])
                .with_resizable(false),
            ..Default::default()
        };
        
        let selected_ip = Arc::new(Mutex::new(None));
        let selected_ip_clone = selected_ip.clone();
        
        let app = LauncherApp::new(&config, selected_ip_clone);
        if let Err(e) = eframe::run_native(
            "RS-View Connection Launcher",
            options,
            Box::new(move |cc| {
                let mut visuals = eframe::egui::Visuals::dark();
                visuals.window_rounding = 8.0.into();
                visuals.widgets.active.rounding = 4.0.into();
                visuals.widgets.hovered.rounding = 4.0.into();
                visuals.widgets.inactive.rounding = 4.0.into();
                cc.egui_ctx.set_visuals(visuals);
                Box::new(app)
            }),
        ) {
            eprintln!("Erro ao iniciar Launcher GUI: {:?}", e);
            return Err(anyhow::anyhow!("Falha no Launcher GUI"));
        }
        
        let ip_opt = {
            let lock = selected_ip.lock().unwrap();
            lock.clone()
        };
        
        if let Some(ip) = ip_opt {
            let mut new_config = LauncherConfig::load();
            new_config.last_ip = ip.clone();
            if !new_config.history.contains(&ip) {
                new_config.history.insert(0, ip.clone());
                if new_config.history.len() > 5 {
                    new_config.history.truncate(5);
                }
            } else {
                if let Some(pos) = new_config.history.iter().position(|x| x == &ip) {
                    new_config.history.remove(pos);
                    new_config.history.insert(0, ip.clone());
                }
            }
            new_config.save();
            (Some(ip), None)
        } else {
            (None, None)
        }
    };

    if let Some(ip) = server_ip {
        run_client(ip, codec_hint)?;
    }

    Ok(())
}

fn run_client(server_ip: String, codec_hint: Option<String>) -> Result<()> {
    sdl2::hint::set("SDL_RENDER_SCALE_QUALITY", "best");

    // Init SDL2
    let sdl_context = sdl2::init().map_err(|e| anyhow::anyhow!(e))?;
    let video_subsystem = sdl_context.video().map_err(|e| anyhow::anyhow!(e))?;

    // Connect to control socket
    let mut control_socket = TcpStream::connect(format!("{}:5001", server_ip))
        .context("Falha ao conectar no socket de controle")?;
    let _ = control_socket.set_nodelay(true);
    let control_socket_read = control_socket.try_clone()
        .context("Falha ao clonar socket de controle")?;

    let (clipboard_tx, clipboard_rx) = mpsc::channel::<String>();
    let latency_ms = Arc::new(Mutex::new(0u32));
    let latency_ms_read = latency_ms.clone();

    thread::spawn(move || {
        let mut reader = std::io::BufReader::new(control_socket_read);
        let mut line = String::new();
        while let Ok(n) = reader.read_line(&mut line) {
            if n == 0 { break; }
            if let Ok(resp) = serde_json::from_str::<ControlResponse>(&line) {
                match resp {
                    ControlResponse::ClipboardSync { text } => {
                        let _ = clipboard_tx.send(text);
                    }
                    ControlResponse::Pong { timestamp } => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        let rtt = now.saturating_sub(timestamp) as u32;
                        if let Ok(mut lock) = latency_ms_read.lock() {
                            *lock = rtt;
                        }
                    }
                }
            }
            line.clear();
        }
    });

    let window = video_subsystem
        .window("RS-View", 1280, 720)
        .position_centered()
        .resizable()
        .build()?;

    let mut canvas = window
        .into_canvas()
        .accelerated()
        .build()?;
    let texture_creator = canvas.texture_creator();
    
    // We will initialize the texture when we receive the first frame.
    let mut texture: Option<sdl2::render::Texture> = None;

    // Buffer atômico de 1 único frame (Zero-Buffer / Auto-Drop)
    let frame_slot = Arc::new(Mutex::new(None));
    let frame_slot_decode = frame_slot.clone();

    // Spawn FFmpeg decode thread
    let server_ip_clone = server_ip.clone();
    let codec_hint_clone = codec_hint.clone();
    thread::spawn(move || {
        if let Err(e) = decode_loop(&server_ip_clone, codec_hint_clone, frame_slot_decode) {
            eprintln!("Erro no decoder FFmpeg: {}", e);
        }
    });

    sdl_context.mouse().show_cursor(true);

    let mut event_pump = sdl_context.event_pump().map_err(|e| anyhow::anyhow!(e))?;

    let mut last_mouse_send = std::time::Instant::now();
    let mut pending_mouse: Option<(i32, i32)> = None;

    // Métricas de FPS e Ping
    let mut frame_count: u32 = 0;
    let mut current_fps: u32 = 0;
    let mut last_fps_time = std::time::Instant::now();
    let mut last_ping_time = std::time::Instant::now();

    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => break 'running,
                Event::MouseMotion { x, y, .. } => {
                    let (win_w, win_h) = canvas.window().size();
                    if win_w > 0 && win_h > 0 {
                        let norm_x = (x as f64 / win_w as f64 * 32767.0) as i32;
                        let norm_y = (y as f64 / win_h as f64 * 32767.0) as i32;
                        pending_mouse = Some((norm_x, norm_y));

                        // Throttling: envia no máximo a cada ~7ms (~140Hz) para evitar saturar o TCP
                        if last_mouse_send.elapsed() >= std::time::Duration::from_millis(7) {
                            if let Some((mx, my)) = pending_mouse.take() {
                                send_cmd(&mut control_socket, InputCommand::MouseMove { x: mx, y: my });
                                last_mouse_send = std::time::Instant::now();
                            }
                        }
                    }
                }
                Event::MouseButtonDown { mouse_btn, .. } => {
                    // Despacha qualquer posição do mouse pendente imediatamente antes do clique
                    if let Some((mx, my)) = pending_mouse.take() {
                        send_cmd(&mut control_socket, InputCommand::MouseMove { x: mx, y: my });
                        last_mouse_send = std::time::Instant::now();
                    }

                    let btn = match mouse_btn {
                        sdl2::mouse::MouseButton::Left => 0,
                        sdl2::mouse::MouseButton::Middle => 1,
                        sdl2::mouse::MouseButton::Right => 2,
                        sdl2::mouse::MouseButton::X1 => 3,
                        sdl2::mouse::MouseButton::X2 => 4,
                        _ => 99,
                    };
                    if btn != 99 {
                        send_cmd(&mut control_socket, InputCommand::MouseButton { button: btn, pressed: true });
                    }
                }
                Event::MouseButtonUp { mouse_btn, .. } => {
                    if let Some((mx, my)) = pending_mouse.take() {
                        send_cmd(&mut control_socket, InputCommand::MouseMove { x: mx, y: my });
                        last_mouse_send = std::time::Instant::now();
                    }

                    let btn = match mouse_btn {
                        sdl2::mouse::MouseButton::Left => 0,
                        sdl2::mouse::MouseButton::Middle => 1,
                        sdl2::mouse::MouseButton::Right => 2,
                        sdl2::mouse::MouseButton::X1 => 3,
                        sdl2::mouse::MouseButton::X2 => 4,
                        _ => 99,
                    };
                    if btn != 99 {
                        send_cmd(&mut control_socket, InputCommand::MouseButton { button: btn, pressed: false });
                    }
                }
                Event::MouseWheel { y, .. } => {
                    send_cmd(&mut control_socket, InputCommand::MouseScroll { dy: y });
                }
                Event::KeyDown { scancode: Some(sc), keymod, .. } => {
                    let ctrl = keymod.intersects(sdl2::keyboard::Mod::LCTRLMOD | sdl2::keyboard::Mod::RCTRLMOD);
                    let gui = keymod.intersects(sdl2::keyboard::Mod::LGUIMOD | sdl2::keyboard::Mod::RGUIMOD);
                    if (ctrl || gui) && sc == Scancode::V {
                        if let Ok(text) = video_subsystem.clipboard().clipboard_text() {
                            send_cmd(&mut control_socket, InputCommand::ClipboardPaste { text });
                        }
                    } else if (ctrl || gui) && sc == Scancode::C {
                        // 1. Simula Ctrl + C no servidor Linux
                        send_cmd(&mut control_socket, InputCommand::Key { code: 29, pressed: true });
                        send_cmd(&mut control_socket, InputCommand::Key { code: 46, pressed: true });
                        send_cmd(&mut control_socket, InputCommand::Key { code: 46, pressed: false });
                        send_cmd(&mut control_socket, InputCommand::Key { code: 29, pressed: false });

                        // 2. Aguarda 150ms e solicita o clipboard remoto
                        let mut control_socket_clone = control_socket.try_clone().unwrap();
                        thread::spawn(move || {
                            thread::sleep(std::time::Duration::from_millis(150));
                            send_cmd(&mut control_socket_clone, InputCommand::ClipboardRequest);
                        });
                    } else {
                        let code = map_scancode_to_linux(sc);
                        if code > 0 {
                            send_cmd(&mut control_socket, InputCommand::Key { code, pressed: true });
                        }
                    }
                }
                Event::KeyUp { scancode: Some(sc), keymod, .. } => {
                    let ctrl = keymod.intersects(sdl2::keyboard::Mod::LCTRLMOD | sdl2::keyboard::Mod::RCTRLMOD);
                    let gui = keymod.intersects(sdl2::keyboard::Mod::LGUIMOD | sdl2::keyboard::Mod::RGUIMOD);
                    if (ctrl || gui) && (sc == Scancode::V || sc == Scancode::C) {
                        // Ignora para não enviar V ou C soltos após colar/copiar
                    } else {
                        let code = map_scancode_to_linux(sc);
                        if code > 0 {
                            send_cmd(&mut control_socket, InputCommand::Key { code, pressed: false });
                        }
                    }
                }
                _ => {}
            }
        }

        // Despacha mouse pendente se o intervalo tiver decorrido
        if pending_mouse.is_some() && last_mouse_send.elapsed() >= std::time::Duration::from_millis(7) {
            if let Some((mx, my)) = pending_mouse.take() {
                send_cmd(&mut control_socket, InputCommand::MouseMove { x: mx, y: my });
                last_mouse_send = std::time::Instant::now();
            }
        }

        // Envia ping de latência a cada 500ms
        if last_ping_time.elapsed() >= std::time::Duration::from_millis(500) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            send_cmd(&mut control_socket, InputCommand::Ping { timestamp: now });
            last_ping_time = std::time::Instant::now();
        }

        // Atualiza cálculo de FPS a cada 1 segundo
        if last_fps_time.elapsed() >= std::time::Duration::from_secs(1) {
            current_fps = frame_count;
            frame_count = 0;
            last_fps_time = std::time::Instant::now();
        }

        // Renderiza exclusivamente o frame mais recente descartando qualquer acúmulo
        let frame_opt = {
            if let Ok(mut lock) = frame_slot.lock() {
                lock.take()
            } else {
                None
            }
        };

        if let Some(frame) = frame_opt {
            frame_count += 1;

            if texture.is_none() || texture.as_ref().unwrap().query().width != frame.width || texture.as_ref().unwrap().query().height != frame.height {
                // Ajusta o tamanho da janela do SDL2 para corresponder ao vídeo nativo do servidor
                let _ = canvas.window_mut().set_size(frame.width, frame.height);

                texture = Some(texture_creator.create_texture_streaming(
                    PixelFormatEnum::IYUV,
                    frame.width,
                    frame.height,
                ).unwrap());
            }

            if let Some(tex) = texture.as_mut() {
                tex.update_yuv(
                    None,
                    &frame.y, frame.y_pitch,
                    &frame.u, frame.u_pitch,
                    &frame.v, frame.v_pitch,
                ).unwrap();

                canvas.clear();
                canvas.copy(tex, None, None).unwrap();

                // ── HUD de FPS e Latência no Canto Superior Direito ───────────
                let rtt = latency_ms.lock().map(|l| *l).unwrap_or(0);
                let hud_text = format!("{} FPS | {} ms", current_fps, rtt);
                let text_scale = 1;
                let char_width = 6 * text_scale;
                let char_height = 7 * text_scale;
                let padding_x = 8;
                let padding_y = 5;
                let dot_size = 6;
                let dot_margin = 6;

                let text_width = hud_text.len() as i32 * char_width;
                let hud_width = padding_x * 2 + dot_size + dot_margin + text_width;
                let hud_height = padding_y * 2 + char_height.max(dot_size);

                let (win_w, _) = canvas.window().size();
                let hud_x = (win_w as i32) - hud_width - 12;
                let hud_y = 12;

                if hud_x > 0 {
                    canvas.set_blend_mode(sdl2::render::BlendMode::Blend);

                    // Fundo translúcido escuro
                    canvas.set_draw_color(sdl2::pixels::Color::RGBA(12, 15, 22, 210));
                    let bg_rect = sdl2::rect::Rect::new(hud_x, hud_y, hud_width as u32, hud_height as u32);
                    let _ = canvas.fill_rect(bg_rect);

                    // Borda fina sutil
                    canvas.set_draw_color(sdl2::pixels::Color::RGBA(70, 85, 110, 180));
                    let _ = canvas.draw_rect(bg_rect);

                    // Indicador LED de latência (Verde <= 15ms | Amarelo <= 40ms | Vermelho > 40ms)
                    let dot_color = if rtt <= 15 {
                        sdl2::pixels::Color::RGB(50, 220, 100)
                    } else if rtt <= 40 {
                        sdl2::pixels::Color::RGB(240, 190, 40)
                    } else {
                        sdl2::pixels::Color::RGB(240, 60, 60)
                    };

                    canvas.set_draw_color(dot_color);
                    let dot_y = hud_y + (hud_height - dot_size) / 2;
                    let dot_rect = sdl2::rect::Rect::new(hud_x + padding_x, dot_y, dot_size as u32, dot_size as u32);
                    let _ = canvas.fill_rect(dot_rect);

                    // Texto Bitmap
                    let text_x = hud_x + padding_x + dot_size + dot_margin;
                    let text_y = hud_y + padding_y;
                    draw_hud_text(&mut canvas, &hud_text, text_x, text_y, sdl2::pixels::Color::RGB(230, 235, 245), text_scale);
                }

                canvas.present();
            }
        } else {
            // Micro-espera para evitar alto uso de CPU ociosa sem introduzir latência perceptível
            thread::sleep(std::time::Duration::from_micros(500));
        }

        // Sincroniza o clipboard local com o recebido do servidor remoto
        while let Ok(text) = clipboard_rx.try_recv() {
            let _ = video_subsystem.clipboard().set_clipboard_text(&text);
        }
    }

    Ok(())
}

fn draw_hud_char(canvas: &mut sdl2::render::Canvas<sdl2::video::Window>, c: char, x: i32, y: i32, color: sdl2::pixels::Color, scale: i32) {
    let bitmap: [u8; 7] = match c {
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'm' => [0b00000, 0b00000, 0b11010, 0b10101, 0b10101, 0b10101, 0b10101],
        's' => [0b00000, 0b00000, 0b01111, 0b10000, 0b01110, 0b00001, 0b11110],
        '|' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        ' ' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
        _ =>   [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
    };

    canvas.set_draw_color(color);
    for (row, bits) in bitmap.iter().enumerate() {
        for col in 0..5 {
            if (bits & (1 << (4 - col))) != 0 {
                let rect = sdl2::rect::Rect::new(
                    x + (col as i32) * scale,
                    y + (row as i32) * scale,
                    scale as u32,
                    scale as u32,
                );
                let _ = canvas.fill_rect(rect);
            }
        }
    }
}

fn draw_hud_text(canvas: &mut sdl2::render::Canvas<sdl2::video::Window>, text: &str, start_x: i32, start_y: i32, color: sdl2::pixels::Color, scale: i32) {
    let mut cur_x = start_x;
    for c in text.chars() {
        draw_hud_char(canvas, c, cur_x, start_y, color, scale);
        cur_x += (5 + 1) * scale;
    }
}

fn send_cmd(socket: &mut TcpStream, cmd: InputCommand) {
    if let Ok(mut json) = serde_json::to_string(&cmd) {
        json.push('\n');
        let _ = socket.write_all(json.as_bytes());
    }
}

fn decode_loop(server_ip: &str, codec_hint: Option<String>, frame_slot: Arc<Mutex<Option<FrameData>>>) -> Result<()> {
    ffmpeg::init()?;
    ffmpeg::log::set_level(ffmpeg::log::Level::Warning);

    if let Some(ref codec) = codec_hint {
        println!("🎥 Iniciando conexão com o servidor. Codec solicitado: {}", codec);
    } else {
        println!("🎥 Iniciando conexão com o servidor. Codec padrão: Autodetectar");
    }

    // Configura ffmpeg para baixa latência em conexões de rede
    let mut dict = ffmpeg::Dictionary::new();
    dict.set("flags", "low_delay");
    dict.set("fflags", "nobuffer");
    dict.set("probesize", "65536");
    dict.set("analyzeduration", "0");

    let input_url = format!("tcp://{}:5000?nodelay=1&buffer_size=65536", server_ip);
    let mut ictx = ffmpeg::format::input_with_dictionary(&input_url, dict)?;

    let input_stream = ictx
        .streams()
        .best(ffmpeg::media::Type::Video)
        .context("Nenhum stream de video encontrado")?;
    let video_stream_index = input_stream.index();

    let mut context_decoder = ffmpeg::codec::context::Context::from_parameters(input_stream.parameters())?;
    context_decoder.set_threading(ffmpeg::codec::threading::Config {
        kind: ffmpeg::codec::threading::Type::None,
        count: 1,
    });
    context_decoder.set_flags(ffmpeg::codec::Flags::LOW_DELAY);
    let mut decoder = context_decoder.decoder().video()?;

    let mut scaler = ffmpeg::software::scaling::context::Context::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        ffmpeg::format::Pixel::YUV420P,
        decoder.width(),
        decoder.height(),
        ffmpeg::software::scaling::flag::Flags::BILINEAR,
    )?;

    let mut receive_and_process_decoded_frames = |decoder: &mut ffmpeg::decoder::Video| -> Result<()> {
        let mut decoded = ffmpeg::frame::Video::empty();
        while decoder.receive_frame(&mut decoded).is_ok() {
            let mut rgb_frame = ffmpeg::frame::Video::empty();
            scaler.run(&decoded, &mut rgb_frame)?;

            let width = rgb_frame.width();
            let height = rgb_frame.height();

            let y_pitch = rgb_frame.stride(0);
            let u_pitch = rgb_frame.stride(1);
            let v_pitch = rgb_frame.stride(2);

            let y_data = rgb_frame.data(0).to_vec();
            let u_data = rgb_frame.data(1).to_vec();
            let v_data = rgb_frame.data(2).to_vec();

            if let Ok(mut lock) = frame_slot.lock() {
                *lock = Some(FrameData {
                    width,
                    height,
                    y: y_data,
                    u: u_data,
                    v: v_data,
                    y_pitch,
                    u_pitch,
                    v_pitch,
                });
            }
        }
        Ok(())
    };

    for (stream, packet) in ictx.packets() {
        if stream.index() == video_stream_index {
            decoder.send_packet(&packet)?;
            receive_and_process_decoded_frames(&mut decoder)?;
        }
    }

    decoder.send_eof()?;
    receive_and_process_decoded_frames(&mut decoder)?;

    Ok(())
}
