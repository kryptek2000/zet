// Ghost autocomplete controller: history-backed suggestion state with
// Tab-accept, Esc-dismiss, and sticky notices. Owns its Store handle so
// shells can hold it without borrow fights (the recorder keeps its own).
// All inputs are plain values (PromptInput); the shell translates grid
// state into them, which keeps this unit-testable and PTY-testable.

use crate::ghost_suffix;
use zet_blocks::Store;

pub struct PromptInput {
    pub text: String,
    pub prompt_col: usize,
    pub prompt_row: usize,
    pub cursor: (usize, usize),
}

pub struct Ghost {
    store: Store,
    last_prefix: String,
    cached: Option<String>,
    dismissed: Option<String>,
    notice_for: Option<String>,
    notice_text: Option<String>,
    shown: Option<String>,
    can_tab: bool,
    /// Did-you-mean offer: a full corrected line, shown on the empty
    /// prompt, Tab-takes it. Previews, never runs by itself.
    preview_text: Option<String>,
}

impl Ghost {
    pub fn new(store: Store) -> Self {
        Ghost {
            store,
            last_prefix: String::new(),
            cached: None,
            dismissed: None,
            notice_for: None,
            notice_text: None,
            shown: None,
            can_tab: false,
            preview_text: None,
        }
    }

    /// Refresh from the current prompt line (None when not typing).
    /// Sync and instant: history only, cached per input.
    pub fn update(&mut self, input: Option<PromptInput>) {
        // Only history suffixes are Tab-completable. Notices display
        // but never type.
        self.can_tab = false;
        let Some(inp) = input else {
            self.shown = None;
            return;
        };
        // Sticky notice while the input matches; cleared the moment it
        // differs so it never resurrects over a later history ghost.
        if self.notice_for.as_deref() == Some(&inp.text) {
            self.shown = self.notice_text.clone();
            return;
        }
        self.notice_for = None;
        self.notice_text = None;
        // Did-you-mean preview: shown on the empty prompt only; any
        // typing supersedes it (history takes over from here).
        if inp.text.is_empty() {
            if let Some(p) = &self.preview_text {
                self.shown = Some(p.clone());
                self.can_tab = true;
            } else {
                self.shown = None;
            }
            return;
        }
        self.preview_text = None;
        if self.dismissed.as_deref() == Some(&inp.text) {
            self.shown = None;
            return;
        }
        if inp.text != self.last_prefix {
            self.last_prefix = inp.text.clone();
            self.cached = self.store.complete(&inp.text).unwrap_or(None);
        }
        let suffix = match &self.cached {
            Some(full) => match ghost_suffix(&inp.text, full) {
                Some(suffix) => suffix.to_string(),
                None => {
                    self.shown = None;
                    return;
                }
            },
            None => {
                self.shown = None;
                return;
            }
        };
        // Cursor at input end only (single-row input; wrapped gets none).
        if inp.cursor != (inp.prompt_col + inp.text.chars().count(), inp.prompt_row) {
            self.shown = None;
            return;
        }
        // Never ghost scrubbed secrets: typing "[REDACTED]" helps nobody
        // and implies the store knows more than it shows.
        if suffix.contains("[REDACTED]") {
            self.shown = None;
            return;
        }
        self.shown = Some(suffix);
        self.can_tab = true;
    }

    /// Visible suffix (after the typed prefix), if any.
    pub fn visible(&self) -> Option<&str> {
        self.shown.as_deref()
    }

    /// True only for a genuine history suffix. Notices, hints, and the
    /// NL "…" fall through to the shell untouched.
    pub fn tabbable(&self) -> bool {
        self.can_tab
    }

    /// Tab: bytes to type into the PTY. None when not completable.
    pub fn accept(&mut self) -> Option<String> {
        if !self.can_tab {
            return None;
        }
        let suffix = self.shown.take()?;
        self.dismissed = None;
        self.preview_text = None;
        self.can_tab = false;
        Some(suffix)
    }

    /// Offer a full corrected line (did-you-mean): shown dimmed on the
    /// empty prompt, Tab-takes it, Esc or typing clears it.
    pub fn preview(&mut self, full: String) {
        if full.trim().is_empty() {
            return;
        }
        self.preview_text = Some(full.clone());
        self.shown = Some(full);
        self.can_tab = true;
    }

    /// Full reset: clears ghost, dismissal, notices, and fix previews.
    /// NL toggle and line-accept use this (fresh context, re-suggest freely).
    pub fn reset(&mut self) {
        self.shown = None;
        self.dismissed = None;
        self.notice_for = None;
        self.notice_text = None;
        self.preview_text = None;
        self.can_tab = false;
    }

    /// Dismiss the current prefix without requiring a visible ghost
    /// (Esc in NL mode with nothing showing still suppresses).
    pub fn dismiss_current(&mut self) {
        self.dismissed = Some(self.last_prefix.clone());
    }

    /// Esc: clears ghost/notice/preview. True = something was showing
    /// (swallow the key); false = pass through to the shell.
    pub fn cancel(&mut self) -> bool {
        let active = self.shown.is_some();
        self.shown = None;
        self.preview_text = None;
        if active {
            self.dismissed = Some(self.last_prefix.clone());
            self.notice_for = None;
            self.notice_text = None;
            return true;
        }
        false
    }

    /// Sticky notice for `input` (e.g. no NL model configured). Shows
    /// until the input changes or Esc.
    pub fn notify(&mut self, input: String, text: String) {
        self.notice_for = Some(input);
        self.notice_text = Some(text.clone());
        self.shown = Some(text);
    }

    /// NL "…" while an answer is in flight. Displays, never types.
    pub fn waiting(&mut self) {
        self.can_tab = false;
        self.shown = Some("…".to_string());
    }
}
