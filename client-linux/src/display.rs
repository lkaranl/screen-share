use anyhow::{Context as AnyhowContext, Result};
use common::command::InputCommand;
use softbuffer::{Context, Surface};
use std::num::NonZeroU32;
use std::sync::Arc;
use tracing::info;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Fullscreen, Window, WindowId};

use crate::control::ControlClient;
use crate::decoder::{DecodedImage, VideoDecoder};
use crate::scancode::winit_key_to_evdev;

pub struct DisplayApp {
    width: u32,
    height: u32,
    decoder: Option<VideoDecoder>,
    control: Option<ControlClient>,
    window: Option<Arc<Window>>,
    surface: Option<Surface<Arc<Window>, Arc<Window>>>,
    is_fullscreen: bool,
    current_frame: Option<DecodedImage>,
}

impl DisplayApp {
    pub fn new(width: u32, height: u32, decoder: VideoDecoder, control: ControlClient) -> Self {
        Self {
            width,
            height,
            decoder: Some(decoder),
            control: Some(control),
            window: None,
            surface: None,
            is_fullscreen: false,
            current_frame: None,
        }
    }

    pub fn run(mut self) -> Result<()> {
        let event_loop = EventLoop::new().context("Falha ao inicializar EventLoop do Winit")?;
        event_loop.set_control_flow(ControlFlow::Poll);
        event_loop.run_app(&mut self).context("Erro no loop de eventos da janela")?;
        Ok(())
    }
}

impl ApplicationHandler for DisplayApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let win_attr = Window::default_attributes()
            .with_title("Screen Share Client (Linux)")
            .with_inner_size(LogicalSize::new(self.width as f64, self.height as f64))
            .with_resizable(true);

        match event_loop.create_window(win_attr) {
            Ok(win) => {
                let window = Arc::new(win);
                match Context::new(window.clone()) {
                    Ok(context) => match Surface::new(&context, window.clone()) {
                        Ok(surface) => {
                            info!(
                                "🖥️ Janela Winit criada com sucesso ({}x{}). F11 alterna tela cheia.",
                                self.width, self.height
                            );
                            self.window = Some(window);
                            self.surface = Some(surface);
                        }
                        Err(e) => info!("Erro ao criar Surface softbuffer: {}", e),
                    },
                    Err(e) => info!("Erro ao criar Context softbuffer: {}", e),
                }
            }
            Err(e) => info!("Falha ao criar janela Winit: {}", e),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                info!("👋 Janela fechada pelo usuário.");
                event_loop.exit();
            }

            WindowEvent::CursorMoved { position, .. } => {
                if let (Some(window), Some(control)) = (&self.window, &self.control) {
                    let win_size = window.inner_size();
                    if win_size.width > 0 && win_size.height > 0 {
                        let norm_x = ((position.x / win_size.width as f64).clamp(0.0, 1.0) * 32767.0) as i32;
                        let norm_y = ((position.y / win_size.height as f64).clamp(0.0, 1.0) * 32767.0) as i32;
                        control.send(InputCommand::MouseMove { x: norm_x, y: norm_y });
                    }
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(control) = &self.control {
                    let btn_idx = match button {
                        MouseButton::Left => 0,
                        MouseButton::Middle => 1,
                        MouseButton::Right => 2,
                        MouseButton::Back => 3,
                        MouseButton::Forward => 4,
                        MouseButton::Other(c) => c as u8,
                    };
                    control.send(InputCommand::MouseButton {
                        button: btn_idx,
                        pressed: state == ElementState::Pressed,
                    });
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(control) = &self.control {
                    let dy = match delta {
                        MouseScrollDelta::LineDelta(_x, y) => (y * 120.0) as i32,
                        MouseScrollDelta::PixelDelta(pos) => pos.y as i32,
                    };
                    if dy != 0 {
                        control.send(InputCommand::MouseScroll { dy });
                    }
                }
            }

            WindowEvent::KeyboardInput { event: key_event, .. } => {
                // F11 para alternar tela cheia
                if let PhysicalKey::Code(KeyCode::F11) = key_event.physical_key {
                    if key_event.state == ElementState::Pressed {
                        if let Some(window) = &self.window {
                            self.is_fullscreen = !self.is_fullscreen;
                            if self.is_fullscreen {
                                window.set_fullscreen(Some(Fullscreen::Borderless(None)));
                            } else {
                                window.set_fullscreen(None);
                            }
                            return;
                        }
                    }
                }

                if let Some(evdev_code) = winit_key_to_evdev(key_event.physical_key) {
                    if let Some(control) = &self.control {
                        control.send(InputCommand::Key {
                            code: evdev_code,
                            pressed: key_event.state == ElementState::Pressed,
                        });
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                if let (Some(window), Some(surface)) = (&self.window, &mut self.surface) {
                    // Verifica se há novos pixels decodificados do stream
                    if let Some(decoder) = &self.decoder {
                        if let Some(frame) = decoder.take_latest_frame() {
                            self.current_frame = Some(frame);
                        }
                    }

                    if let Some(frame) = &self.current_frame {
                        let win_size = window.inner_size();
                        if let (Some(w), Some(h)) = (NonZeroU32::new(win_size.width), NonZeroU32::new(win_size.height)) {
                            let _ = surface.resize(w, h);
                            if let Ok(mut buffer) = surface.buffer_mut() {
                                let target_len = (win_size.width * win_size.height) as usize;
                                if frame.pixels.len() == target_len {
                                    buffer.copy_from_slice(&frame.pixels);
                                } else {
                                    // Se o tamanho da janela for diferente da resolução do stream,
                                    // realiza amostragem rápida / escala direta no buffer
                                    let src_w = frame.width;
                                    let src_h = frame.height;
                                    let dst_w = win_size.width as usize;
                                    let dst_h = win_size.height as usize;

                                    for y in 0..dst_h {
                                        let src_y = (y * src_h) / dst_h;
                                        let dst_offset = y * dst_w;
                                        let src_offset = src_y * src_w;
                                        for x in 0..dst_w {
                                            let src_x = (x * src_w) / dst_w;
                                            if src_offset + src_x < frame.pixels.len() && dst_offset + x < buffer.len() {
                                                buffer[dst_offset + x] = frame.pixels[src_offset + src_x];
                                            }
                                        }
                                    }
                                }
                                let _ = buffer.present();
                            }
                        }
                    }
                }
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            // Solicita novo redraw contínuo para taxa de quadros suave
            window.request_redraw();
        }
    }
}
