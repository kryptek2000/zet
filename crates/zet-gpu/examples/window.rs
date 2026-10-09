// Smoke test: open a ZET window, clear N frames, exit.
// Usage: cargo run -p zet-gpu --example window -- 120
// (Headless CI only compiles this; run it where a display exists.)

use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::Window;
use zet_gpu::Renderer;

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    frames_left: u32,
    rendered: u32,
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
                if self.frames_left == 0 {
                    event_loop.exit();
                } else {
                    if let Some(r) = &mut self.renderer {
                        r.render();
                        self.rendered += 1;
                    }
                    self.frames_left -= 1;
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
        .unwrap_or(120);
    let event_loop = EventLoop::new().unwrap();
    let mut app = App {
        window: None,
        renderer: None,
        frames_left: frames,
        rendered: 0,
    };
    event_loop.run_app(&mut app).unwrap();
    eprintln!("zet-window: rendered {} frames, clean exit", app.rendered);
    assert_eq!(app.rendered, frames, "frame count mismatch");
}
