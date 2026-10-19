use zet_term::Event;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Prompt,
    Input,
    Running,
}

pub struct ShellState {
    phase: Phase,
    last_exit: Option<u32>,
    /// Cwd reported by an OSC 633 `P;Cwd=…` marker, if any. `None` when
    /// the shell never reported one (then the caller's `/proc` lookup is
    /// the source of truth, as before). Cwd is the one thing a marker can
    /// carry that the prompt text cannot, so it lives here rather than in
    /// the phase machine.
    cwd: Option<String>,
}

impl ShellState {
    pub fn new() -> Self {
        ShellState {
            phase: Phase::Prompt,
            last_exit: None,
            cwd: None,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn last_exit(&self) -> Option<u32> {
        self.last_exit
    }

    /// Cwd reported by the shell, if it sent `OSC 633 P;Cwd=…`.
    pub fn cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    pub fn observe(&mut self, event: &Event) {
        // Single-arm match (not let/if-let): exhaustive today, and a
        // compile error here — not silent misbehavior — if Event grows.
        let (code, body) = match event {
            Event::Osc { code, body } => (code, body),
        };
        if *code != 133 && *code != 633 {
            return;
        }
        let (kind, arg) = match body.split_once(';') {
            Some((k, a)) => (k, Some(a)),
            None => (body.as_str(), None),
        };
        match kind {
            "A" => self.phase = Phase::Prompt,
            "B" => self.phase = Phase::Input,
            "C" => self.phase = Phase::Running,
            "D" => {
                self.phase = Phase::Prompt;
                if let Some(a) = arg {
                    if let Ok(code) = a.parse::<u32>() {
                        self.last_exit = Some(code);
                    }
                }
            }
            _ => {}
        }
        // OSC 633 carries properties the 133 stream cannot. Only `P;Cwd=`
        // is consumed today; unknown properties are ignored, and a missing
        // Cwd leaves the prior value untouched (one marker can carry several).
        if *code == 633 && kind == "P" {
            if let Some(params) = arg {
                for param in params.split(';') {
                    if let Some(value) = param.strip_prefix("Cwd=") {
                        if !value.is_empty() {
                            self.cwd = Some(value.to_string());
                        }
                    }
                }
            }
        }
    }
}

impl Default for ShellState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zet_term::Event;

    /// OSC 133 `D;code` is already covered by the integration tests; this
    /// pins the 633 Cwd payload that no hook currently emits, so the parser
    /// stays ready and a future emitter cannot silently break the contract.
    #[test]
    fn cwd_is_captured_from_633_marker() {
        let mut st = ShellState::new();
        assert_eq!(st.cwd(), None);

        st.observe(&Event::Osc {
            code: 633,
            body: "P;Cwd=/home/bandit/zet".to_string(),
        });
        assert_eq!(st.cwd(), Some("/home/bandit/zet"));

        // A later marker without Cwd must not clobber the known cwd.
        st.observe(&Event::Osc {
            code: 633,
            body: "P;Foo=bar".to_string(),
        });
        assert_eq!(st.cwd(), Some("/home/bandit/zet"));

        // Cwd updates take effect on the next report.
        st.observe(&Event::Osc {
            code: 633,
            body: "P;Cwd=/tmp;Foo=bar".to_string(),
        });
        assert_eq!(st.cwd(), Some("/tmp"));

        // 133 markers never carry cwd.
        st.observe(&Event::Osc {
            code: 133,
            body: "D;0".to_string(),
        });
        assert_eq!(st.cwd(), Some("/tmp"));
    }

    #[test]
    fn empty_cwd_payload_is_ignored() {
        let mut st = ShellState::new();
        st.observe(&Event::Osc {
            code: 633,
            body: "P;Cwd=".to_string(),
        });
        assert_eq!(st.cwd(), None);
    }

    /// A byte stream shaped like zsh's hook output (which we cannot run here
    /// because zsh is not installed in this environment) drives the same
    /// parser and the same exit-code tracker. This is the closest headless
    /// proof available; the live zsh test in shell_state.rs is gated on the
    /// binary being present.
    #[test]
    fn zsh_shaped_marker_stream_reports_exit_and_cwd() {
        let mut t = zet_term::Terminal::new(80, 24);
        let stream = b"\x1b]133;A\x07\x1b]133;B\x07false\n\x1b]133;C\x07\x1b]133;D;1\x07\
                      \x1b]133;A\x07\x1b]133;B\x07true\n\x1b]133;C\x07\x1b]133;D;0\x07\
                      \x1b]633;P;Cwd=/home/bandit/zet\x07";
        t.feed(stream);
        let mut st = ShellState::new();
        for e in t.drain_events() {
            st.observe(&e);
        }
        assert_eq!(st.last_exit(), Some(0));
        assert_eq!(st.cwd(), Some("/home/bandit/zet"));
        assert_eq!(st.phase(), Phase::Prompt);
    }
}
