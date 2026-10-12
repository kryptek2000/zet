use winit::keyboard::{Key, ModifiersState, NamedKey};
use zet_gpu::input::{paste_bytes, translate};

fn plain() -> ModifiersState {
    ModifiersState::empty()
}

fn ctrl() -> ModifiersState {
    ModifiersState::CONTROL
}

fn ctrl_shift() -> ModifiersState {
    ModifiersState::CONTROL | ModifiersState::SHIFT
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

#[test]
fn backspace_as_text_maps_to_del() {
    // Some platforms report Backspace as U+0008 text instead of Named.
    assert_eq!(
        translate(
            &Key::Character("\u{8}".into()),
            Some("\u{8}"),
            true,
            &plain()
        ),
        Some(vec![0x7f])
    );
}

#[test]
fn paste_chords_are_detected_not_typed() {
    use zet_gpu::input::is_paste;
    // Ctrl+Shift+V and Shift+Insert paste.
    assert!(is_paste(&Key::Character("V".into()), true, &ctrl_shift()));
    assert!(is_paste(&Key::Character("v".into()), true, &ctrl_shift()));
    assert!(is_paste(
        &Key::Named(NamedKey::Insert),
        true,
        &ModifiersState::SHIFT
    ));
    // Nearby chords are not paste: Ctrl+V is a literal control char,
    // plain V types, releases never paste.
    assert!(!is_paste(&Key::Character("v".into()), true, &ctrl()));
    assert!(!is_paste(&Key::Character("v".into()), true, &plain()));
    assert!(!is_paste(&Key::Character("V".into()), false, &ctrl_shift()));
    assert!(!is_paste(&Key::Named(NamedKey::Insert), true, &plain()));
}

#[test]
fn paste_normalizes_newlines_to_cr() {
    assert_eq!(paste_bytes("abc"), b"abc");
    assert_eq!(paste_bytes("a\nb\r\nc\rd"), b"a\rb\rc\rd");
    assert_eq!(paste_bytes(""), b"");
}

#[test]
fn copy_chord_is_detected_not_typed() {
    use zet_gpu::input::is_copy;
    // Ctrl+Shift+C copies; bare Ctrl+C stays SIGINT, plain C types.
    assert!(is_copy(&Key::Character("C".into()), true, &ctrl_shift()));
    assert!(is_copy(&Key::Character("c".into()), true, &ctrl_shift()));
    assert!(!is_copy(&Key::Character("c".into()), true, &ctrl()));
    assert!(!is_copy(&Key::Character("c".into()), true, &plain()));
    assert!(!is_copy(&Key::Character("C".into()), false, &ctrl_shift()));
}

#[test]
fn ctrl_g_toggles_agent_not_bell() {
    use zet_gpu::input::is_agent_toggle;
    // Ctrl+G would type BEL (0x07) via translate; in the shell it must
    // open goal mode instead. Plain g types; releases never toggle.
    assert!(is_agent_toggle(&Key::Character("g".into()), true, &ctrl()));
    assert!(is_agent_toggle(&Key::Character("G".into()), true, &ctrl()));
    assert!(!is_agent_toggle(
        &Key::Character("g".into()),
        true,
        &plain()
    ));
    assert!(!is_agent_toggle(
        &Key::Character("g".into()),
        false,
        &ctrl()
    ));
}

#[test]
fn bracketed_wrap_keeps_newlines_literal() {
    use zet_gpu::input::paste_wrapped;
    // Bracketed: newlines become CR inside the 200~/201~ envelope so
    // readline inserts instead of executing.
    assert_eq!(
        paste_wrapped("a\nb\r\nc", true),
        b"\x1b[200~a\rb\rc\x1b[201~".to_vec()
    );
    // Unset: legacy raw CRs (shell executes).
    assert_eq!(paste_wrapped("a\nb", false), b"a\rb".to_vec());
}
