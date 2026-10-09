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
}

impl ShellState {
    pub fn new() -> Self {
        ShellState {
            phase: Phase::Prompt,
            last_exit: None,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn last_exit(&self) -> Option<u32> {
        self.last_exit
    }

    pub fn observe(&mut self, event: &Event) {
        let Event::Osc { code, body } = event;
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
                self.last_exit = arg.and_then(|a| a.parse::<u32>().ok());
            }
            _ => {}
        }
    }
}

impl Default for ShellState {
    fn default() -> Self {
        Self::new()
    }
}
