// Keyboard translation: winit key events -> PTY bytes.
// Kept free of winit event plumbing (takes Key/text/pressed/modifiers)
// so the mapping is unit-testable without a window.

use winit::keyboard::{Key, ModifiersState, NamedKey};

/// True when this press is a paste chord (Ctrl+Shift+V, Shift+Insert),
/// not text to type. Check before [`translate`]: with Ctrl held, V would
/// otherwise become a literal SYN byte (0x16).
pub fn is_paste(logical: &Key, pressed: bool, mods: &ModifiersState) -> bool {
    if !pressed {
        return false;
    }
    let ctrl = mods.contains(ModifiersState::CONTROL);
    let shift = mods.contains(ModifiersState::SHIFT);
    match logical {
        Key::Character(s) if ctrl && shift && s.eq_ignore_ascii_case("v") => true,
        Key::Named(NamedKey::Insert) if shift && !ctrl => true,
        _ => false,
    }
}

/// Clipboard text as PTY bytes: lone LF and CRLF both become CR, bare CR
/// passes through. (Bracketed paste, mode 2004, is a follow-up: pastes
/// execute line-by-line until then, like any non-bracketed terminal.)
pub fn paste_bytes(text: &str) -> Vec<u8> {
    text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
}

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
