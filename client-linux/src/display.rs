use anyhow::{Context, Result};
use common::command::InputCommand;
use sdl2::event::Event;
use sdl2::mouse::MouseButton as SdlMouseButton;
use sdl2::pixels::PixelFormatEnum;
use sdl2::render::TextureAccess;
use std::time::Duration;
use tracing::info;

use crate::control::ControlClient;
use crate::decoder::VideoDecoder;
use crate::scancode::sdl_scancode_to_evdev;

pub struct DisplayWindow {
    sdl_context: sdl2::Sdl,
    video_subsystem: sdl2::VideoSubsystem,
    width: u32,
    height: u32,
}

impl DisplayWindow {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        let sdl_context = sdl2::init().map_err(|e| anyhow::anyhow!("Erro ao inicializar SDL2: {}", e))?;
        let video_subsystem = sdl_context.video().map_err(|e| anyhow::anyhow!("Erro ao inicializar SDL2 Video: {}", e))?;

        Ok(Self {
            sdl_context,
            video_subsystem,
            width,
            height,
        })
    }

    pub fn run(
        self,
        decoder: VideoDecoder,
        control: ControlClient,
    ) -> Result<()> {
        let window = self
            .video_subsystem
            .window("Screen Share Client (Linux)", self.width, self.height)
            .position_centered()
            .resizable()
            .build()
            .context("Falha ao criar janela SDL2")?;

        let mut canvas = window
            .into_canvas()
            .accelerated()
            .present_vsync()
            .build()
            .context("Falha ao criar canvas acelerado SDL2")?;

        let texture_creator = canvas.texture_creator();
        let mut texture = texture_creator
            .create_texture(
                PixelFormatEnum::IYUV,
                TextureAccess::Streaming,
                self.width,
                self.height,
            )
            .context("Falha ao criar textura IYUV no SDL2")?;

        let mut event_pump = self
            .sdl_context
            .event_pump()
            .map_err(|e| anyhow::anyhow!("Falha ao obter event pump SDL2: {}", e))?;

        info!("🖥️ Janela de exibição iniciada ({}x{}). F11 para alternar tela cheia.", self.width, self.height);

        let mut is_fullscreen = false;
        let y_plane_size = (self.width * self.height) as usize;
        let uv_plane_size = y_plane_size / 4;

        'main_loop: loop {
            // 1. Processamento de eventos de input (Mouse e Teclado)
            for event in event_pump.poll_iter() {
                match event {
                    Event::Quit { .. } => {
                        info!("👋 Janela fechada pelo usuário.");
                        break 'main_loop;
                    }

                    Event::KeyDown { scancode: Some(scancode), keycode: _, repeat: false, .. } => {
                        // F11: Alternar tela cheia
                        if scancode == sdl2::keyboard::Scancode::F11 {
                            is_fullscreen = !is_fullscreen;
                            let mode = if is_fullscreen {
                                sdl2::video::FullscreenType::Desktop
                            } else {
                                sdl2::video::FullscreenType::Off
                            };
                            let _ = canvas.window_mut().set_fullscreen(mode);
                            continue;
                        }

                        if let Some(evdev_code) = sdl_scancode_to_evdev(scancode) {
                            control.send(InputCommand::Key {
                                code: evdev_code,
                                pressed: true,
                            });
                        }
                    }

                    Event::KeyUp { scancode: Some(scancode), repeat: false, .. } => {
                        if let Some(evdev_code) = sdl_scancode_to_evdev(scancode) {
                            control.send(InputCommand::Key {
                                code: evdev_code,
                                pressed: false,
                            });
                        }
                    }

                    Event::MouseMotion { x, y, xrel, yrel, .. } => {
                        let (win_w, win_h) = canvas.window().size();
                        if win_w > 0 && win_h > 0 {
                            // Coordenadas normalizadas 0..32767
                            let norm_x = ((x as f32 / win_w as f32).clamp(0.0, 1.0) * 32767.0) as i32;
                            let norm_y = ((y as f32 / win_h as f32).clamp(0.0, 1.0) * 32767.0) as i32;
                            control.send(InputCommand::MouseMove { x: norm_x, y: norm_y });

                            if xrel != 0 || yrel != 0 {
                                control.send(InputCommand::MouseMoveRelative {
                                    dx: xrel,
                                    dy: yrel,
                                });
                            }
                        }
                    }

                    Event::MouseButtonDown { mouse_btn, .. } => {
                        let button_idx = match mouse_btn {
                            SdlMouseButton::Left => 0,
                            SdlMouseButton::Middle => 1,
                            SdlMouseButton::Right => 2,
                            SdlMouseButton::X1 => 3,
                            SdlMouseButton::X2 => 4,
                            _ => continue,
                        };
                        control.send(InputCommand::MouseButton {
                            button: button_idx,
                            pressed: true,
                        });
                    }

                    Event::MouseButtonUp { mouse_btn, .. } => {
                        let button_idx = match mouse_btn {
                            SdlMouseButton::Left => 0,
                            SdlMouseButton::Middle => 1,
                            SdlMouseButton::Right => 2,
                            SdlMouseButton::X1 => 3,
                            SdlMouseButton::X2 => 4,
                            _ => continue,
                        };
                        control.send(InputCommand::MouseButton {
                            button: button_idx,
                            pressed: false,
                        });
                    }

                    Event::MouseWheel { y, .. } => {
                        // Scroll: positivo = cima, negativo = baixo
                        control.send(InputCommand::MouseScroll { dy: y });
                    }

                    _ => {}
                }
            }

            // 2. Renderização de Vídeo: verifica se há novo frame decodificado
            if let Some(frame) = decoder.take_latest_frame() {
                if frame.data.len() >= y_plane_size + uv_plane_size * 2 {
                    let y_plane = &frame.data[0..y_plane_size];
                    let u_plane = &frame.data[y_plane_size..y_plane_size + uv_plane_size];
                    let v_plane = &frame.data[y_plane_size + uv_plane_size..y_plane_size + uv_plane_size * 2];

                    let _ = texture.update_yuv(
                        None,
                        y_plane,
                        self.width as usize,
                        u_plane,
                        (self.width / 2) as usize,
                        v_plane,
                        (self.width / 2) as usize,
                    );

                    canvas.clear();
                    let _ = canvas.copy(&texture, None, None);
                    canvas.present();
                }
            } else {
                // Pequena pausa para evitar 100% de CPU quando ocioso
                std::thread::sleep(Duration::from_millis(1));
            }
        }

        Ok(())
    }
}
