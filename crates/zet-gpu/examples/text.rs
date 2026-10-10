// Text smoke test: render a styled zet-term grid through the GPU pipeline.
// Usage: cargo run -p zet-gpu --example text -- [frames]
// 0 frames (default) = stay open until closed. This is the human smoke test.

use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::Window;
use zet_gpu::Renderer;
use zet_term::Terminal;

const COLS: usize = 80;
const ROWS: usize = 24;

fn demo_term() -> Terminal {
    let mut t = Terminal::new(COLS, ROWS);
    // Column-0 marker: bright-white bg block proves the first cell renders.
    t.feed(b"\x1b[107m \x1b[0m\x1b[1;36mZET \x1b[0m\x1b[32mterminal emulator\x1b[0m\r\n");
    t.feed(b"----------------------------------------\r\n");
    t.feed(b"\x1b[33m$ \x1b[0m\x1b[37mecho hello\x1b[0m\r\n");
    t.feed(b"hello\r\n");
    t.feed(b"\x1b[31mH\x1b[33me\x1b[32ml\x1b[36ml\x1b[34mo\x1b[35m \x1b[37mW\x1b[90mo\x1b[92mr\x1b[93ml\x1b[95md\x1b[0m\r\n");
    t.feed(b"The quick brown fox jumps over 1234567890\r\n");
    t.feed(b"\x1b[41;97m RED BACKGROUND \x1b[0m plain\r\n");
    t
}

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    term: Terminal,
    frames_left: u32,
    rendered: u32,
    persistent: bool,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes().with_title("ZET"))
                .unwrap(),
        );
        let renderer = pollster::block_on(Renderer::new(window.clone())).unwrap();
        self.window = Some(window);
        self.renderer = Some(renderer);
        event_loop.set_control_flow(ControlFlow::Poll);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.renderer {
                    r.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                if self.frames_left == 0 && !self.persistent {
                    event_loop.exit();
                } else {
                    if let Some(r) = &mut self.renderer {
                        r.draw_terminal(&self.term, COLS, ROWS);
                        r.render();
                        self.rendered += 1;
                    }
                    if !self.persistent {
                        self.frames_left -= 1;
                    }
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let frames: u32 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(0);
    let event_loop = EventLoop::new().unwrap();
    let mut app = App {
        window: None,
        renderer: None,
        term: demo_term(),
        frames_left: frames,
        rendered: 0,
        persistent: frames == 0,
    };
    event_loop.run_app(&mut app).unwrap();
    if !app.persistent {
        eprintln!("zet-text: rendered {} frames, clean exit", app.rendered);
        assert_eq!(app.rendered, frames, "frame count mismatch");
    }
}
