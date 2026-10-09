// wgpu renderer skeleton: window surface, device, per-frame clear.
// Text rasterization (glyph atlas upload) is the next slice; this proves
// the GPU path end-to-end: init -> configure -> resize -> present.

use std::fmt;
use std::sync::Arc;
use winit::window::Window;

#[derive(Debug)]
pub enum InitError {
    NoAdapter,
    NoSurfaceConfig,
    Device(wgpu::RequestDeviceError),
    Surface(wgpu::CreateSurfaceError),
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InitError::NoAdapter => write!(f, "no suitable GPU adapter found"),
            InitError::NoSurfaceConfig => write!(f, "no surface configuration available"),
            InitError::Device(e) => write!(f, "device request failed: {e}"),
            InitError::Surface(e) => write!(f, "surface creation failed: {e}"),
        }
    }
}

impl std::error::Error for InitError {}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    bg: wgpu::Color,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Result<Self, InitError> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window)
            .map_err(InitError::Surface)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .map_err(|_| InitError::NoAdapter)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(InitError::Device)?;
        let config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or(InitError::NoSurfaceConfig)?;
        surface.configure(&device, &config);
        Ok(Renderer {
            surface,
            device,
            queue,
            config,
            bg: wgpu::Color {
                r: 0.008,
                g: 0.012,
                b: 0.027,
                a: 1.0,
            },
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    pub fn render(&mut self) {
        use wgpu::CurrentSurfaceTexture as Cst;
        let frame = match self.surface.get_current_texture() {
            Cst::Success(frame) | Cst::Suboptimal(frame) => frame,
            Cst::Outdated | Cst::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Cst::Timeout | Cst::Occluded | Cst::Validation => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("zet-frame"),
            });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("zet-clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.bg),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
    }
}
