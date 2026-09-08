use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use anyhow::{Context as AnyhowContext, Result};
use softbuffer::{Context, Surface};
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, PixmapMut, Rect, Transform};
use tracing::info;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

const FONT_DATA: &[u8] = include_bytes!("../assets/font.ttf");

#[derive(Clone)]
pub struct ServerUiState {
    pub local_ip: Arc<Mutex<String>>,
    pub video_port: u16,
    pub control_port: u16,
    pub uinput_active: bool,
    pub active_clients: Arc<AtomicU32>,
    pub is_running: Arc<AtomicBool>,
    pub active_codec: Arc<AtomicU8>,
}

pub struct ServerControlPanel {
    state: ServerUiState,
    window: Option<Arc<Window>>,
    surface: Option<Surface<Arc<Window>, Arc<Window>>>,
    width: u32,
    height: u32,
    copied_until: Option<Instant>,
    mouse_pos: (f64, f64),
    // Retângulos dos botões (para cliques e hover)
    copy_btn_rect: Option<(f32, f32, f32, f32)>,
    stop_btn_rect: Option<(f32, f32, f32, f32)>,
}

impl ServerControlPanel {
    pub fn new(state: ServerUiState) -> Self {
        Self {
            state,
            window: None,
            surface: None,
            width: 540,
            height: 420,
            copied_until: None,
            mouse_pos: (0.0, 0.0),
            copy_btn_rect: None,
            stop_btn_rect: None,
        }
    }

    pub fn run(mut self) -> Result<()> {
        let event_loop = EventLoop::new().context("Falha ao inicializar EventLoop da interface gráfica")?;
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(100)));
        event_loop.run_app(&mut self).context("Erro no loop de eventos do painel do servidor")?;
        Ok(())
    }

    fn render(&mut self) {
        let (w, h) = match &self.window {
            Some(win) => {
                let size = win.inner_size();
                (size.width, size.height)
            }
            None => return,
        };
        if w == 0 || h == 0 {
            return;
        }

        let mut pixmap = match Pixmap::new(w, h) {
            Some(p) => p,
            None => return,
        };

        self.draw_ui(&mut pixmap.as_mut(), w as f32, h as f32);

        if let Some(surface) = &mut self.surface {
            if let (Some(nz_w), Some(nz_h)) = (NonZeroU32::new(w), NonZeroU32::new(h)) {
                let _ = surface.resize(nz_w, nz_h);
                if let Ok(mut buffer) = surface.buffer_mut() {
                    for (src, dst) in pixmap.data().chunks_exact(4).zip(buffer.iter_mut()) {
                        let r = src[0] as u32;
                        let g = src[1] as u32;
                        let b = src[2] as u32;
                        *dst = (r << 16) | (g << 8) | b;
                    }
                    let _ = buffer.present();
                }
            }
        }
    }

    fn draw_ui(&mut self, pixmap: &mut PixmapMut, w: f32, h: f32) {
        let font = match FontRef::try_from_slice(FONT_DATA) {
            Ok(f) => f,
            Err(_) => return,
        };

        // 1. Fundo Geral (Dark Slate Moderno)
        pixmap.fill(Color::from_rgba8(18, 20, 26, 255));

        // 2. Barra Superior / Título
        draw_text(pixmap, &font, "Screen Share Server", 32.0, 42.0, 24.0, Color::from_rgba8(255, 255, 255, 255));
        draw_text(
            pixmap,
            &font,
            "Compartilhamento de Tela de Baixa Latência (Linux)",
            32.0,
            66.0,
            13.0,
            Color::from_rgba8(148, 163, 184, 255),
        );

        // 3. Card Principal de Status
        let card_x = 30.0;
        let card_y = 90.0;
        let card_w = w - 60.0;
        let card_h = 240.0;

        // Fundo do Card
        let mut card_pb = PathBuilder::new();
        card_pb.push_rect(Rect::from_xywh(card_x, card_y, card_w, card_h).unwrap_or(Rect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap()));
        if let Some(path) = card_pb.finish() {
            let mut paint = Paint::default();
            paint.set_color_rgba8(28, 32, 43, 255);
            paint.anti_alias = true;
            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);

            // Borda do Card
            let mut border_paint = Paint::default();
            border_paint.set_color_rgba8(42, 48, 66, 255);
            let mut stroke = tiny_skia::Stroke::default();
            stroke.width = 1.0;
            pixmap.stroke_path(&path, &border_paint, &stroke, Transform::identity(), None);
        }

        // Indicador de Status com Círculo Colorido e Halo
        let clients = self.state.active_clients.load(Ordering::Relaxed);
        let is_running = self.state.is_running.load(Ordering::Relaxed);

        let (circle_color, halo_color, status_title, status_desc) = if !is_running {
            (
                Color::from_rgba8(239, 68, 68, 255), // Vermelho
                Color::from_rgba8(239, 68, 68, 50),
                "SERVIDOR ENCERRADO",
                "O serviço de compartilhamento foi interrompido.",
            )
        } else if clients > 0 {
            (
                Color::from_rgba8(59, 130, 246, 255), // Azul vibrante
                Color::from_rgba8(59, 130, 246, 50),
                "CLIENTE CONECTADO",
                "Transmitindo tela em tempo real com baixa latência.",
            )
        } else if !self.state.uinput_active {
            (
                Color::from_rgba8(245, 158, 11, 255), // Amarelo
                Color::from_rgba8(245, 158, 11, 50),
                "ONLINE (SEM CONTROLE DE INPUT)",
                "Vídeo ativo, mas /dev/uinput requer permissão de escrita.",
            )
        } else {
            (
                Color::from_rgba8(16, 185, 129, 255), // Verde brilhante
                Color::from_rgba8(16, 185, 129, 50),
                "ONLINE - PRONTO PARA CONEXÕES",
                "O servidor está escutando na rede local e pronto.",
            )
        };

        // Halo pulsante do círculo
        let circle_cx = card_x + 30.0;
        let circle_cy = card_y + 36.0;

        let mut halo_pb = PathBuilder::new();
        halo_pb.push_circle(circle_cx, circle_cy, 12.0);
        if let Some(path) = halo_pb.finish() {
            let mut paint = Paint::default();
            paint.set_color(halo_color);
            paint.anti_alias = true;
            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        }

        // Círculo Central de Status
        let mut dot_pb = PathBuilder::new();
        dot_pb.push_circle(circle_cx, circle_cy, 6.0);
        if let Some(path) = dot_pb.finish() {
            let mut paint = Paint::default();
            paint.set_color(circle_color);
            paint.anti_alias = true;
            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        }

        // Textos de Status
        draw_text(pixmap, &font, status_title, card_x + 54.0, card_y + 32.0, 16.0, circle_color);
        draw_text(pixmap, &font, status_desc, card_x + 54.0, card_y + 50.0, 12.0, Color::from_rgba8(148, 163, 184, 255));

        // Linha divisória dentro do Card
        let mut div_pb = PathBuilder::new();
        div_pb.move_to(card_x + 20.0, card_y + 70.0);
        div_pb.line_to(card_x + card_w - 20.0, card_y + 70.0);
        if let Some(path) = div_pb.finish() {
            let mut stroke = tiny_skia::Stroke::default();
            stroke.width = 1.0;
            let mut paint = Paint::default();
            paint.set_color_rgba8(42, 48, 66, 255);
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }

        // Informações Detalhadas
        let local_ip = self.state.local_ip.lock().unwrap().clone();
        let codec_name = if self.state.active_codec.load(Ordering::Relaxed) == 0 { "H.264 (AVC)" } else { "HEVC (H.265)" };

        let info_y = card_y + 100.0;
        let line_spacing = 30.0;

        // Linha 1: IP de Conexão
        draw_text(pixmap, &font, "IP da Máquina:", card_x + 24.0, info_y, 14.0, Color::from_rgba8(148, 163, 184, 255));
        draw_text(pixmap, &font, &local_ip, card_x + 160.0, info_y, 15.0, Color::from_rgba8(248, 250, 252, 255));

        // Botão Pequeno Copiar ao lado do IP
        let copy_btn_x = card_x + card_w - 120.0;
        let copy_btn_y = info_y - 18.0;
        let copy_btn_w = 96.0;
        let copy_btn_h = 26.0;
        self.copy_btn_rect = Some((copy_btn_x, copy_btn_y, copy_btn_w, copy_btn_h));

        let is_hover_copy = self.is_hover(copy_btn_x, copy_btn_y, copy_btn_w, copy_btn_h);
        let is_copied = self.copied_until.map(|t| Instant::now() < t).unwrap_or(false);

        let (copy_btn_bg, copy_btn_label) = if is_copied {
            (Color::from_rgba8(16, 185, 129, 255), "Copiado!")
        } else if is_hover_copy {
            (Color::from_rgba8(37, 99, 235, 255), "Copiar IP")
        } else {
            (Color::from_rgba8(30, 41, 59, 255), "Copiar IP")
        };

        draw_button(pixmap, copy_btn_x, copy_btn_y, copy_btn_w, copy_btn_h, copy_btn_bg);
        draw_text(
            pixmap,
            &font,
            copy_btn_label,
            copy_btn_x + 18.0,
            copy_btn_y + 18.0,
            12.0,
            Color::from_rgba8(255, 255, 255, 255),
        );

        // Linha 2: Portas de Rede
        draw_text(pixmap, &font, "Portas de Rede:", card_x + 24.0, info_y + line_spacing, 14.0, Color::from_rgba8(148, 163, 184, 255));
        let ports_text = format!("{} (Vídeo UDP)  /  {} (Controle TCP)", self.state.video_port, self.state.control_port);
        draw_text(pixmap, &font, &ports_text, card_x + 160.0, info_y + line_spacing, 14.0, Color::from_rgba8(226, 232, 240, 255));

        // Linha 3: Controle Remoto
        draw_text(pixmap, &font, "Controle Remoto:", card_x + 24.0, info_y + line_spacing * 2.0, 14.0, Color::from_rgba8(148, 163, 184, 255));
        let (uinput_text, uinput_color) = if self.state.uinput_active {
            ("Habilitado (/dev/uinput pronto)", Color::from_rgba8(52, 211, 153, 255))
        } else {
            ("Desabilitado (Requer permissão)", Color::from_rgba8(248, 113, 113, 255))
        };
        draw_text(pixmap, &font, uinput_text, card_x + 160.0, info_y + line_spacing * 2.0, 14.0, uinput_color);

        // Linha 4: Conexões e Codec
        draw_text(pixmap, &font, "Sessão Atual:", card_x + 24.0, info_y + line_spacing * 3.0, 14.0, Color::from_rgba8(148, 163, 184, 255));
        let session_text = format!("{} cliente(s) ativo(s)  •  Codec: {}", clients, codec_name);
        draw_text(pixmap, &font, &session_text, card_x + 160.0, info_y + line_spacing * 3.0, 14.0, Color::from_rgba8(226, 232, 240, 255));

        // 4. Rodapé de Ações
        let stop_btn_w = 150.0;
        let stop_btn_h = 36.0;
        let stop_btn_x = w - card_x - stop_btn_w;
        let stop_btn_y = h - 60.0;
        self.stop_btn_rect = Some((stop_btn_x, stop_btn_y, stop_btn_w, stop_btn_h));

        let is_hover_stop = self.is_hover(stop_btn_x, stop_btn_y, stop_btn_w, stop_btn_h);
        let stop_btn_bg = if is_hover_stop {
            Color::from_rgba8(220, 38, 38, 255) // Vermelho mais vivo no hover
        } else {
            Color::from_rgba8(185, 28, 28, 255)
        };

        draw_button(pixmap, stop_btn_x, stop_btn_y, stop_btn_w, stop_btn_h, stop_btn_bg);
        draw_text(
            pixmap,
            &font,
            "Parar Servidor",
            stop_btn_x + 26.0,
            stop_btn_y + 23.0,
            14.0,
            Color::from_rgba8(255, 255, 255, 255),
        );
    }

    fn is_hover(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        let mx = self.mouse_pos.0 as f32;
        let my = self.mouse_pos.1 as f32;
        mx >= x && mx <= x + w && my >= y && my <= y + h
    }
}

impl ApplicationHandler for ServerControlPanel {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let win_attr = Window::default_attributes()
            .with_title("Screen Share - Servidor")
            .with_inner_size(LogicalSize::new(self.width as f64, self.height as f64))
            .with_resizable(false);

        match event_loop.create_window(win_attr) {
            Ok(win) => {
                let window = Arc::new(win);
                match Context::new(window.clone()) {
                    Ok(context) => match Surface::new(&context, window.clone()) {
                        Ok(surface) => {
                            info!("🖥️ Painel gráfico do servidor inicializado com sucesso.");
                            self.window = Some(window);
                            self.surface = Some(surface);
                            self.render();
                        }
                        Err(e) => info!("Erro ao criar Surface softbuffer no painel: {}", e),
                    },
                    Err(e) => info!("Erro ao criar Context softbuffer no painel: {}", e),
                }
            }
            Err(e) => info!("Falha ao criar janela do painel do servidor: {}", e),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                info!("👋 Painel do servidor fechado pelo usuário. Encerrando servidor...");
                self.state.is_running.store(false, Ordering::SeqCst);
                event_loop.exit();
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_pos = (position.x, position.y);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }

            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                // Clique no botão "Copiar IP"
                if let Some((x, y, w, h)) = self.copy_btn_rect {
                    if self.is_hover(x, y, w, h) {
                        let ip = self.state.local_ip.lock().unwrap().clone();
                        let _ = common::clipboard::set_system_clipboard(&ip);
                        self.copied_until = Some(Instant::now() + Duration::from_secs(2));
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                        return;
                    }
                }

                // Clique no botão "Parar Servidor"
                if let Some((x, y, w, h)) = self.stop_btn_rect {
                    if self.is_hover(x, y, w, h) {
                        info!("🛑 Botão 'Parar Servidor' pressionado. Finalizando...");
                        self.state.is_running.store(false, Ordering::SeqCst);
                        event_loop.exit();
                        return;
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                self.render();
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if !self.state.is_running.load(Ordering::Relaxed) {
            event_loop.exit();
            return;
        }

        // Se a janela estiver visível, agenda redesenho regular para atualizar status e animações
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(200)));
    }
}

fn draw_button(pixmap: &mut PixmapMut, x: f32, y: f32, w: f32, h: f32, color: Color) {
    let mut pb = PathBuilder::new();
    pb.push_rect(Rect::from_xywh(x, y, w, h).unwrap_or(Rect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap()));
    if let Some(path) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color(color);
        paint.anti_alias = true;
        pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }
}

fn draw_text(
    pixmap: &mut PixmapMut,
    font: &FontRef,
    text: &str,
    x: f32,
    y: f32,
    scale: f32,
    color: Color,
) {
    let scaled_font = font.as_scaled(PxScale::from(scale));
    let mut cursor_x = x;

    for ch in text.chars() {
        let glyph = scaled_font.scaled_glyph(ch);
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, c| {
                let px = (cursor_x + bounds.min.x + gx as f32) as i32;
                let py = (y + bounds.min.y + gy as f32) as i32;
                if px >= 0 && px < pixmap.width() as i32 && py >= 0 && py < pixmap.height() as i32 {
                    let pixel_index = (py as usize * pixmap.width() as usize + px as usize) * 4;
                    let slice = &mut pixmap.data_mut()[pixel_index..pixel_index + 4];
                    let alpha = c * color.alpha();
                    let inv_alpha = 1.0 - alpha;
                    let r = (color.red() * 255.0 * alpha + slice[0] as f32 * inv_alpha) as u8;
                    let g = (color.green() * 255.0 * alpha + slice[1] as f32 * inv_alpha) as u8;
                    let b = (color.blue() * 255.0 * alpha + slice[2] as f32 * inv_alpha) as u8;
                    slice[0] = r;
                    slice[1] = g;
                    slice[2] = b;
                    slice[3] = 255;
                }
            });
        }
        cursor_x += scaled_font.h_advance(font.glyph_id(ch));
    }
}
