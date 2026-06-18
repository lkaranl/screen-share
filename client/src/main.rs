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
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ControlResponse {
    ClipboardSync { text: String },
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
        .present_vsync()
        .build()?;
    let texture_creator = canvas.texture_creator();
    
    // We will initialize the texture when we receive the first frame.
    let mut texture: Option<sdl2::render::Texture> = None;

    let (frame_tx, frame_rx) = mpsc::channel::<FrameData>();

    // Spawn FFmpeg decode thread
    let server_ip_clone = server_ip.clone();
    let codec_hint_clone = codec_hint.clone();
    thread::spawn(move || {
        if let Err(e) = decode_loop(&server_ip_clone, codec_hint_clone, frame_tx) {
            eprintln!("Erro no decoder FFmpeg: {}", e);
        }
    });

    sdl_context.mouse().show_cursor(true);
    // sdl_context.mouse().set_relative_mouse_mode(true); // Capture mouse perfectly (uncomment for full capture)

    let mut event_pump = sdl_context.event_pump().map_err(|e| anyhow::anyhow!(e))?;

    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => break 'running,
                Event::MouseMotion { x, y, .. } => {
                    let (win_w, win_h) = canvas.window().size();
                    if win_w > 0 && win_h > 0 {
                        let norm_x = (x as f64 / win_w as f64 * 32767.0) as i32;
                        let norm_y = (y as f64 / win_h as f64 * 32767.0) as i32;
                        send_cmd(&mut control_socket, InputCommand::MouseMove { x: norm_x, y: norm_y });
                    }
                }
                Event::MouseButtonDown { mouse_btn, .. } => {
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

        // Render any available frames
        let mut has_new_frame = false;
        while let Ok(frame) = frame_rx.try_recv() {
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
                has_new_frame = true;
            }
        }

        if has_new_frame {
            if let Some(tex) = texture.as_ref() {
                canvas.clear();
                canvas.copy(tex, None, None).unwrap();
                canvas.present();
            }
        }

        // Sincroniza o clipboard local com o recebido do servidor remota
        while let Ok(text) = clipboard_rx.try_recv() {
            let _ = video_subsystem.clipboard().set_clipboard_text(&text);
        }

        thread::sleep(std::time::Duration::from_millis(2));
    }

    Ok(())
}

fn send_cmd(socket: &mut TcpStream, cmd: InputCommand) {
    if let Ok(mut json) = serde_json::to_string(&cmd) {
        json.push('\n');
        let _ = socket.write_all(json.as_bytes());
    }
}

fn decode_loop(_server_ip: &str, codec_hint: Option<String>, frame_tx: mpsc::Sender<FrameData>) -> Result<()> {
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
    // HEVC tem frames IDR maiores. probesize 4096 trunca o frame na análise
    // e causa "Could not find ref". Aumentar para 128KB resolve isso sem atraso.
    dict.set("probesize", "131072");
    dict.set("analyzeduration", "0");

    let input_url = "udp://0.0.0.0:5000?fifo_size=1000000&overrun_nonfatal=1".to_string();
    let mut ictx = ffmpeg::format::input_with_dictionary(&input_url, dict)?;

    let input_stream = ictx
        .streams()
        .best(ffmpeg::media::Type::Video)
        .context("Nenhum stream de video encontrado")?;
    let video_stream_index = input_stream.index();

    let context_decoder = ffmpeg::codec::context::Context::from_parameters(input_stream.parameters())?;
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

            if frame_tx.send(FrameData {
                width,
                height,
                y: y_data,
                u: u_data,
                v: v_data,
                y_pitch,
                u_pitch,
                v_pitch,
            }).is_err() {
                return Ok(()); // Main thread closed
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
