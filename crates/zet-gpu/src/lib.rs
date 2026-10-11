pub mod atlas;
pub mod damage;
pub mod input;
pub mod quad;
pub mod renderer;
pub mod text;

pub use renderer::Renderer;

/// Cold-start stage mark (ZET_PERF=1 only): ms since the first mark in
/// the process. Env-gated; silent otherwise.
pub fn perf_mark(label: &str) {
    use std::sync::OnceLock;
    static T0: OnceLock<std::time::Instant> = OnceLock::new();
    if std::env::var_os("ZET_PERF").is_none() {
        return;
    }
    let t0 = T0.get_or_init(std::time::Instant::now);
    eprintln!("perf: mark {label} {}ms", t0.elapsed().as_millis());
}
