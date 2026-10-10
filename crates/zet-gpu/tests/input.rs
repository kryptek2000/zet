use winit::keyboard::{Key, ModifiersState, NamedKey};
use zet_gpu::input::translate;

fn plain() -> ModifiersState {
    ModifiersState::empty()
}

fn ctrl() -> ModifiersState {
    ModifiersState::CONTROL
}

#[test]
fn text_enter_and_control_codes() {
    assert_eq!(
        translate(&Key::Character("a".into()), Some("a"), true, &plain()),
        Some(b"a".to_vec())
    );
    assert_eq!(
        translate(&Key::Named(NamedKey::Enter), Some("\r"), true, &plain()),
        Some(b"\r".to_vec())
    );
    assert_eq!(
        translate(&Key::Character("c".into()), None, true, &ctrl()),
        Some(vec![0x03])
    );
    // Release events never produce bytes.
    assert_eq!(
        translate(&Key::Character("a".into()), Some("a"), false, &plain()),
        None
    );
}

#[test]
fn navigation_keys_emit_escape_sequences() {
    let cases = [
        (NamedKey::ArrowUp, "\x1b[A"),
        (NamedKey::ArrowDown, "\x1b[B"),
        (NamedKey::ArrowRight, "\x1b[C"),
        (NamedKey::ArrowLeft, "\x1b[D"),
        (NamedKey::Home, "\x1b[H"),
        (NamedKey::End, "\x1b[F"),
        (NamedKey::Delete, "\x1b[3~"),
        (NamedKey::Backspace, "\x7f"),
        (NamedKey::Tab, "\t"),
        (NamedKey::Escape, "\x1b"),
    ];
    for (named, want) in cases {
        assert_eq!(
            translate(&Key::Named(named), None, true, &plain()),
            Some(want.as_bytes().to_vec()),
            "{named:?}"
        );
    }
}
