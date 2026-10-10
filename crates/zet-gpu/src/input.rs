// Keyboard translation: winit key events -> PTY bytes.
// Kept free of winit event plumbing (takes Key/text/pressed/modifiers)
// so the mapping is unit-testable without a window.

use winit::keyboard::{Key, ModifiersState, NamedKey};

/// Translate a key press into bytes for the PTY.
/// Returns None for releases and unmapped keys.
pub fn translate(
    logical: &Key,
    text: Option<&str>,
    pressed: bool,
    mods: &ModifiersState,
) -> Option<Vec<u8>> {
    if !pressed {
        return None;
    }
    if mods.contains(ModifiersState::CONTROL) {
        if let Key::Character(s) = logical {
            if let Some(c) = s.chars().next() {
                let lower = c.to_ascii_lowercase() as u8;
                if lower.is_ascii_lowercase() {
                    return Some(vec![lower & 0x1f]);
                }
            }
        }
        return None;
    }
    match logical {
        Key::Named(NamedKey::Enter) => Some(b"\r".to_vec()),
        Key::Named(NamedKey::Backspace) => Some(b"\x7f".to_vec()),
        Key::Named(NamedKey::Tab) => Some(b"\t".to_vec()),
        Key::Named(NamedKey::Escape) => Some(b"\x1b".to_vec()),
        Key::Named(NamedKey::ArrowUp) => Some(b"\x1b[A".to_vec()),
        Key::Named(NamedKey::ArrowDown) => Some(b"\x1b[B".to_vec()),
        Key::Named(NamedKey::ArrowRight) => Some(b"\x1b[C".to_vec()),
        Key::Named(NamedKey::ArrowLeft) => Some(b"\x1b[D".to_vec()),
        Key::Named(NamedKey::Home) => Some(b"\x1b[H".to_vec()),
        Key::Named(NamedKey::End) => Some(b"\x1b[F".to_vec()),
        Key::Named(NamedKey::Delete) => Some(b"\x1b[3~".to_vec()),
        Key::Character(s) if &**s == "\u{8}" => Some(b"\x7f".to_vec()),
        _ => text.map(|t| t.as_bytes().to_vec()),
    }
}
