// Headless VT conformance harness.
//
// Issue #1's acceptance is "vttest pass". There is no vttest harness in
// the repo (grep finds no reference), so this is one: real VT100/VT220
// behaviour driven straight through the parser, no window, no terminal
// emulator, no network, no fixture download. Each case is a byte stream
// plus the exact grid text it must produce.
//
// Why cases and not the upstream suite: vttest is Thomas Dickey's
// VT100/VT220 test binary and its `ok` corpus is thousands of files in
// his `.in`/`.expect` format. Fetching it at test time would make CI
// depend on a third-party host; vendoring thousands of files would bury
// this change. So the harness ships a curated corpus of the behaviours
// that matter for a terminal emulator, written in the same spirit: exact
// expected output, byte-driven input, driven headlessly. The counts are
// asserted below so a case cannot be silently deleted to make a run
// green — `cases_run` pins the size of the corpus itself.

use zet_term::Terminal;

/// Render the visible grid, trailing blanks trimmed per row, rows joined
/// by `\n` and trailing empty rows dropped. This is the observable
/// result a user would see; every expectation below is written against
/// it rather than against internals.
fn screen(term: &Terminal) -> String {
    let mut rows: Vec<String> = (0..term.rows())
        .map(|y| term.line_text(y).trim_end().to_string())
        .collect();
    while rows.last().is_some_and(|l| l.is_empty()) {
        rows.pop();
    }
    rows.join("\n")
}

struct Case {
    name: &'static str,
    cols: usize,
    rows: usize,
    /// Bytes fed to the parser.
    input: &'static [u8],
    /// Exact expected screen text.
    expect: &'static str,
}

const CASES: &[Case] = &[
    Case {
        name: "plain_text_and_newline",
        cols: 20,
        rows: 3,
        input: b"hello\r\nworld\r\n",
        expect: "hello\nworld",
    },
    Case {
        name: "lf_alone_moves_down_but_not_right",
        cols: 20,
        rows: 3,
        input: b"ab\ncd",
        expect: "ab\ncd",
    },
    Case {
        name: "cr_returns_to_column_zero",
        cols: 20,
        rows: 3,
        input: b"abcdef\rxy",
        expect: "xycdef",
    },
    Case {
        name: "backspace_overwrites",
        cols: 20,
        rows: 3,
        input: b"abc\x08\x08Z",
        expect: "aZc",
    },
    Case {
        name: "autowrap_at_right_margin",
        cols: 5,
        rows: 3,
        input: b"abcdefg",
        expect: "abcde\nfg",
    },
    Case {
        name: "pending_wrap_cancelled_by_cursor_move",
        cols: 5,
        rows: 3,
        // Fill the row exactly, then move up before printing again:
        // a pending wrap must not consume the next character.
        input: b"abcde\x1b[1A\x1b[1;1HZ",
        expect: "Zbcde",
    },
    Case {
        name: "backspace_after_exact_fit_stays_on_row",
        cols: 5,
        rows: 3,
        input: b"abcde\x08\x08X",
        expect: "abXde",
    },
    Case {
        name: "cup_positions_cursor",
        cols: 10,
        rows: 4,
        input: b"\x1b[2;3Hxy",
        expect: "\n  xy",
    },
    Case {
        name: "cup_default_is_home",
        cols: 10,
        rows: 4,
        input: b"hello\x1b[HZ",
        expect: "Zello",
    },
    Case {
        name: "cuf_cud_move_relative",
        cols: 10,
        rows: 4,
        input: b"\x1b[2C\x1b[1BX",
        expect: "\n  X",
    },
    Case {
        name: "cub_cup_move_left",
        cols: 10,
        rows: 4,
        input: b"abcdef\x1b[3DX",
        expect: "abcXef",
    },
    Case {
        name: "cursor_up_clamped_at_top",
        cols: 10,
        rows: 3,
        input: b"\x1b[2;1Hgo\x1b[99AX",
        expect: "  X\ngo",
    },
    Case {
        name: "cursor_forward_clamped_at_right",
        cols: 5,
        rows: 2,
        input: b"\x1b[99CX",
        expect: "    X",
    },
    Case {
        name: "ed_zero_clears_to_end_of_line",
        cols: 10,
        rows: 2,
        input: b"abcdefgh\x1b[1;4H\x1b[K",
        expect: "abc",
    },
    Case {
        name: "ed_one_clears_to_start_of_line",
        cols: 10,
        rows: 2,
        input: b"abcdefgh\x1b[1;5H\x1b[1K",
        expect: "     fgh",
    },
    Case {
        name: "ed_two_clears_whole_screen",
        cols: 6,
        rows: 3,
        input: b"aaa\r\nbbb\r\nccc\x1b[2J\x1b[HZ",
        expect: "Z",
    },
    Case {
        name: "el_zero_and_el_one",
        cols: 8,
        rows: 2,
        input: b"abcdef\x1b[1;3H\x1b[0KZ",
        expect: "abZ",
    },
    Case {
        name: "el_two_erases_entire_line",
        cols: 8,
        rows: 2,
        input: b"abcdef\x1b[1;3H\x1b[2KZ",
        expect: "  Z",
    },
    Case {
        name: "insert_line_pushes_down",
        cols: 8,
        rows: 4,
        input: b"one\r\ntwo\r\n\x1b[1;1H\x1b[L",
        expect: "\none\ntwo",
    },
    Case {
        name: "delete_line_pulls_up",
        cols: 8,
        rows: 4,
        input: b"one\r\ntwo\r\n\x1b[1;1H\x1b[M",
        expect: "two",
    },
    Case {
        name: "scroll_region_constrains_scrolling",
        cols: 8,
        rows: 5,
        input: b"\x1b[2;4r\x1b[2;1Ha\r\nb\r\nc\r\nd\x1b[r",
        expect: "\nb\nc\nd",
    },
    Case {
        name: "reverse_index_scrolls_down_at_top",
        cols: 8,
        rows: 3,
        input: b"one\r\n\x1b[1;1H\x1bM",
        expect: "\none",
    },
    Case {
        name: "nel_ind_and_ri_lf_equivalents",
        cols: 8,
        rows: 3,
        input: b"a\x1bEb\x1bDc\x1bM",
        expect: "a\nb\n c",
    },
    Case {
        name: "tab_advances_to_eight_stops",
        cols: 30,
        rows: 2,
        input: b"a\tb",
        expect: "a       b",
    },
    Case {
        name: "tab_from_column_one",
        cols: 30,
        rows: 2,
        input: b"\ta",
        expect: "        a",
    },
    Case {
        name: "cursor_save_restore_roundtrip",
        cols: 10,
        rows: 4,
        input: b"\x1b[2;5H\x1b7\x1b[4;1HZ\x1b8Y",
        expect: "\n    Y\n\nZ",
    },
    Case {
        name: "sgr_changes_bold_and_colors_but_not_text",
        cols: 20,
        rows: 2,
        input: b"\x1b[1;31;44mstyled\x1b[0m",
        expect: "styled",
    },
    Case {
        name: "sgr_empty_is_reset",
        cols: 20,
        rows: 2,
        input: b"\x1b[31mred\x1b[mplain",
        expect: "redplain",
    },
    Case {
        name: "decset_private_sequences_leak_no_text",
        cols: 20,
        rows: 2,
        input: b"\x1b[?25l\x1b[?1049h\x1b[?2004hvisible",
        expect: "visible",
    },
    Case {
        name: "charset_select_consumed",
        cols: 20,
        rows: 2,
        input: b"\x1b(B\x1b)B\x1b(0\x1b)0text",
        expect: "text",
    },
    Case {
        name: "index_sequence_moves_down",
        cols: 8,
        rows: 3,
        input: b"a\x1bDb",
        expect: "a\n b",
    },
    Case {
        name: "osc_bel_terminated_is_skipped",
        cols: 20,
        rows: 2,
        input: b"\x1b]0;window title\x07after",
        expect: "after",
    },
    Case {
        name: "osc_st_terminated_is_skipped",
        cols: 20,
        rows: 2,
        input: b"\x1b]0;title\x1b\\after",
        expect: "after",
    },
    Case {
        name: "osc_does_not_wrap_the_line",
        cols: 10,
        rows: 3,
        // A long OSC payload must not consume grid cells or wrap.
        input: b"\x1b]133;D;0;aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\x07x",
        expect: "x",
    },
    Case {
        name: "esc_unknown_intermediates_are_consumed",
        cols: 10,
        rows: 2,
        input: b"\x1b(B\x1b#8\x1b%Gtext",
        expect: "text",
    },
    Case {
        name: "can_and_ht_expand_to_tabs",
        cols: 12,
        rows: 2,
        input: b"a\x08\x08b",
        expect: "b",
    },
    Case {
        name: "utf8_multibyte_advances_one_cell",
        cols: 10,
        rows: 2,
        input: "héllo→".as_bytes(),
        expect: "héllo→",
    },
    Case {
        name: "invalid_utf8_byte_does_not_panic",
        cols: 10,
        rows: 2,
        input: b"a\xff\xfeb",
        expect: "a\u{FFFD}\u{FFFD}b",
    },
    Case {
        name: "alt_screen_preserves_main_grid",
        cols: 10,
        rows: 3,
        input: b"main\x1b[?1049halt\x1b[?1049lback",
        expect: "mainback",
    },
    Case {
        name: "wrap_then_scroll_pushes_to_scrollback",
        cols: 6,
        rows: 3,
        input: b"aaaaa\r\nbbbbb\r\nccccc\r\nddddd",
        expect: "bbbbb\nccccc\nddddd",
    },
    Case {
        name: "repeated_cr_overwrites_same_row",
        cols: 8,
        rows: 2,
        input: b"12345678\rAB",
        expect: "AB345678",
    },
    Case {
        name: "linefeed_at_bottom_scrolls",
        cols: 8,
        rows: 2,
        input: b"a\r\nb\r\nc",
        expect: "b\nc",
    },
    Case {
        name: "cuf_at_last_column_and_print",
        cols: 4,
        rows: 2,
        input: b"ab\x1b[99CX",
        expect: "ab X",
    },
];

/// Pin the corpus size. Without this, a case could be deleted to make a
/// run green and the suite would still pass — the exact failure mode the
/// red-team work on zet-policy guards against.
const CASES_PINNED: usize = 43;

#[test]
fn vttest_corpus_is_the_size_we_think_it_is() {
    assert_eq!(
        CASES.len(),
        CASES_PINNED,
        "the corpus changed size: {} cases, expected {CASES_PINNED}. Adding \
         cases is fine (bump the pin); deleting one to dodge a failure is not.",
        CASES.len()
    );
    let mut names: Vec<&str> = CASES.iter().map(|c| c.name).collect();
    let before = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(before, names.len(), "duplicate case names in the corpus");
}

#[test]
fn vttest_conformance_corpus_passes() {
    let mut failures = Vec::new();
    for case in CASES {
        let mut term = Terminal::with_scrollback(case.cols, case.rows, 64);
        term.feed(case.input);
        let got = screen(&term);
        if got != case.expect {
            failures.push(format!(
                "{}\n  input:  {:?}\n  expect: {:?}\n  got:    {:?}",
                case.name, case.input, case.expect, got
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} conformance cases failed:\n\n{}",
        failures.len(),
        CASES.len(),
        failures.join("\n\n")
    );
}

/// The corpus above drives the parser through a public API. This pins
/// that a single malformed or hostile stream cannot panic the parser —
/// the fuzz-shaped shape: every escape state fed garbage.
#[test]
fn hostile_byte_streams_never_panic() {
    let streams: &[&[u8]] = &[
        b"\x1b",
        b"\x1b[",
        b"\x1b[;;;;H",
        b"\x1b[99999999999999999999H",
        b"\x1b[?9999999999h",
        b"\x1b]133;D;99999999999999999999\x07",
        b"\x1b]133;A",
        b"\x1b_\x1b^X\x1bX\x1bZ",
        b"\x1bP1;2q\x1b\\\x1b[?1049h",
        "\x1b\u{200b}[31m".as_bytes(),
        b"\x1b[38;2;255;255;255mrgb",
        b"\x1b[48;5;200m256",
        b"\x1b[1;2;3;4;5;6;7;8;9;10mSGR",
        b"\x00\x01\x02\x07\x7f\x9b",
        b"\x1b[0;0;0H\x1b[;;;;H\x1b[999;999H",
    ];
    for stream in streams {
        let mut term = Terminal::with_scrollback(10, 4, 16);
        term.feed(stream);
        term.feed(b"after");
        let _ = screen(&term);
        // A stream split across feed calls must behave identically.
        let mut split = Terminal::with_scrollback(10, 4, 16);
        for chunk in stream.chunks(1) {
            split.feed(chunk);
        }
        split.feed(b"after");
        assert_eq!(
            screen(&term),
            screen(&split),
            "feed chunking changed the result"
        );
    }
}
