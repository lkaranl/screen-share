use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use sdl2::event::Event;
use sdl2::keyboard::Scancode;
use sdl2::pixels::PixelFormatEnum;
use serde::{Deserialize, Serialize};
use std::env;
use std::io::Write;
use std::net::TcpStream;
use std::sync::mpsc;
use std::thread;

#[derive(Debug, Serialize, Deserialize)]
pub enum InputCommand {
    MouseMove { x: i32, y: i32 },
    MouseButton { button: u8, pressed: bool },
    MouseScroll { dy: i32 },
    Key { code: u16, pressed: bool },
}

// Map SDL2 Scancode to Linux Input Event Keycode
fn map_scancode_to_linux(scancode: Scancode) -> u16 {
    // Basic mapping, just the essentials for testing
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

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: {} <server_ip>", args[0]);
        return Ok(());
    }
    let server_ip = args[1].clone();

    // Connect to control socket
    let mut control_socket = TcpStream::connect(format!("{}:5001", server_ip))
        .context("Falha ao conectar no socket de controle")?;

    // Init SDL2
    let sdl_context = sdl2::init().map_err(|e| anyhow::anyhow!(e))?;
    let video_subsystem = sdl_context.video().map_err(|e| anyhow::anyhow!(e))?;

    let window = video_subsystem
        .window("Screen Share Client", 1280, 720)
        .position_centered()
        .resizable()
        .build()?;

    let mut canvas = window.into_canvas().build()?;
    let texture_creator = canvas.texture_creator();
    
    // We will initialize the texture when we receive the first frame.
    let mut texture: Option<sdl2::render::Texture> = None;

    let (frame_tx, frame_rx) = mpsc::channel::<FrameData>();

    // Spawn FFmpeg decode thread
    thread::spawn(move || {
        if let Err(e) = decode_loop(&server_ip, frame_tx) {
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
                Event::KeyDown { scancode: Some(sc), .. } => {
                    let code = map_scancode_to_linux(sc);
                    if code > 0 {
                        send_cmd(&mut control_socket, InputCommand::Key { code, pressed: true });
                    }
                }
                Event::KeyUp { scancode: Some(sc), .. } => {
                    let code = map_scancode_to_linux(sc);
                    if code > 0 {
                        send_cmd(&mut control_socket, InputCommand::Key { code, pressed: false });
                    }
                }
                _ => {}
            }
        }

        // Render any available frames
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
            }
        }

        if let Some(tex) = texture.as_ref() {
            canvas.clear();
            canvas.copy(tex, None, None).unwrap();
            canvas.present();
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

fn decode_loop(server_ip: &str, frame_tx: mpsc::Sender<FrameData>) -> Result<()> {
    ffmpeg::init()?;
    ffmpeg::log::set_level(ffmpeg::log::Level::Error);

    // Configura ffmpeg para baixa latência em conexões de rede
    let mut dict = ffmpeg::Dictionary::new();
    dict.set("flags", "low_delay");
    dict.set("fflags", "nobuffer");
    dict.set("probesize", "32");
    dict.set("analyzeduration", "0");

    let input_url = format!("tcp://{}:5000", server_ip);
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
