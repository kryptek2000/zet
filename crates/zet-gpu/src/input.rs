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

/// Paste bytes honoring bracketed-paste mode: when the hosted app set
/// DECSET 2004, wrap in 200~/201~ with newlines as CR so readline
/// inserts the paste literally instead of executing each line.
pub fn paste_wrapped(text: &str, bracketed: bool) -> Vec<u8> {
    if !bracketed {
        return paste_bytes(text);
    }
    let mut out = Vec::with_capacity(text.len() + 12);
    out.extend_from_slice(b"\x1b[200~");
    out.extend_from_slice(text.replace("\r\n", "\r").replace('\n', "\r").as_bytes());
    out.extend_from_slice(b"\x1b[201~");
    out
}

/// True when this press copies the mouse selection (Ctrl+Shift+C),
/// not text to type. Check before [`translate`]: with Ctrl held, C would
/// otherwise become ETX (0x03) — SIGINT, the opposite of copying.
pub fn is_copy(logical: &Key, pressed: bool, mods: &ModifiersState) -> bool {
    if !pressed {
        return false;
    }
    let ctrl = mods.contains(ModifiersState::CONTROL);
    let shift = mods.contains(ModifiersState::SHIFT);
    matches!(logical, Key::Character(s) if ctrl && shift && s.eq_ignore_ascii_case("c"))
}

/// True when this press selects all terminal text (Ctrl+Shift+A).
/// Bare Ctrl+A stays readline's line-start; plain A types.
pub fn is_select_all(logical: &Key, pressed: bool, mods: &ModifiersState) -> bool {
    if !pressed {
        return false;
    }
    let ctrl = mods.contains(ModifiersState::CONTROL);
    let shift = mods.contains(ModifiersState::SHIFT);
    matches!(logical, Key::Character(s) if ctrl && shift && s.eq_ignore_ascii_case("a"))
}

/// True when this press toggles agent goal mode (Ctrl+G). Check before
/// [`translate`]: Ctrl+G would otherwise become a BEL byte (0x07).
pub fn is_agent_toggle(logical: &Key, pressed: bool, mods: &ModifiersState) -> bool {
    if !pressed {
        return false;
    }
    if !mods.contains(ModifiersState::CONTROL) {
        return false;
    }
    matches!(logical, Key::Character(s) if s.eq_ignore_ascii_case("g"))
}

/// True on Ctrl+Shift+T: open a new tab. Checked before [`translate`].
pub fn is_new_tab(logical: &Key, pressed: bool, mods: &ModifiersState) -> bool {
    if !pressed {
        return false;
    }
    let ctrl = mods.contains(ModifiersState::CONTROL);
    let shift = mods.contains(ModifiersState::SHIFT);
    matches!(logical, Key::Character(s) if ctrl && shift && s.eq_ignore_ascii_case("t"))
}

/// True on Ctrl+Shift+W: close the active tab. Checked before [`translate`].
pub fn is_close_tab(logical: &Key, pressed: bool, mods: &ModifiersState) -> bool {
    if !pressed {
        return false;
    }
    let ctrl = mods.contains(ModifiersState::CONTROL);
    let shift = mods.contains(ModifiersState::SHIFT);
    matches!(logical, Key::Character(s) if ctrl && shift && s.eq_ignore_ascii_case("w"))
}

/// True on Ctrl+Shift+F: find in scrollback. Checked before [`translate`].
pub fn is_search(logical: &Key, pressed: bool, mods: &ModifiersState) -> bool {
    if !pressed {
        return false;
    }
    let ctrl = mods.contains(ModifiersState::CONTROL);
    let shift = mods.contains(ModifiersState::SHIFT);
    matches!(logical, Key::Character(s) if ctrl && shift && s.eq_ignore_ascii_case("f"))
}

/// True on Ctrl+Shift+M: toggle the Mission Rail. Checked before
/// [`translate`] so the key never reaches the shell.
pub fn is_rail_toggle(logical: &Key, pressed: bool, mods: &ModifiersState) -> bool {
    if !pressed {
        return false;
    }
    let ctrl = mods.contains(ModifiersState::CONTROL);
    let shift = mods.contains(ModifiersState::SHIFT);
    matches!(logical, Key::Character(s) if ctrl && shift && s.eq_ignore_ascii_case("m"))
}

/// True on Ctrl+Shift+I: toggle the Contextual Inspector. Checked before
/// [`translate`] so the key never reaches the shell.
pub fn is_inspector_toggle(logical: &Key, pressed: bool, mods: &ModifiersState) -> bool {
    if !pressed {
        return false;
    }
    let ctrl = mods.contains(ModifiersState::CONTROL);
    let shift = mods.contains(ModifiersState::SHIFT);
    matches!(logical, Key::Character(s) if ctrl && shift && s.eq_ignore_ascii_case("i"))
}

/// True on Ctrl+PageDown / Ctrl+PageUp: cycle tabs. Checked before
/// [`translate`] (which would emit scrolling escapes).
pub fn is_cycle_tab(logical: &Key, pressed: bool, mods: &ModifiersState) -> Option<isize> {
    if !pressed {
        return None;
    }
    if !mods.contains(ModifiersState::CONTROL) {
        return None;
    }
    match logical {
        Key::Named(NamedKey::PageDown) => Some(1),
        Key::Named(NamedKey::PageUp) => Some(-1),
        _ => None,
    }
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
