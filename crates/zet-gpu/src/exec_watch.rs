//! Stall watch for agent steps typed into the live shell.
//!
//! A step that records no block may be a long command — or a wait on
//! stdin that never comes (a pager, or `cat` aliased to `bat` with the
//! filename lost). Abandoning it leaves the shell stuck AND lets later
//! steps pile into the waiter's stdin as mystery rows. So the watch
//! interrupts once (^C reclaims the prompt; the 130 block usually
//! settles right after) and only abandons when the prompt never returns.

use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
pub enum StepPoll {
    /// A block settled for this step: complete normally.
    Settle,
    /// Nothing yet, still inside budget: keep waiting.
    Wait,
    /// Budget spent with nothing recorded: send ^C now (once).
    Interrupt,
    /// ^C sent and grace spent: give up so the loop survives.
    Abandon,
}

pub struct StepWatch {
    timeout: Duration,
    grace: Duration,
    interrupted: bool,
}

impl StepWatch {
    pub fn new(timeout: Duration, grace: Duration) -> Self {
        StepWatch {
            timeout,
            grace,
            interrupted: false,
        }
    }

    pub fn interrupted(&self) -> bool {
        self.interrupted
    }

    /// `elapsed` since the step was typed, `settled` when a block
    /// recorded past the step's starting count.
    pub fn poll(&mut self, elapsed: Duration, settled: bool) -> StepPoll {
        if settled {
            return StepPoll::Settle;
        }
        if elapsed < self.timeout {
            return StepPoll::Wait;
        }
        if !self.interrupted {
            self.interrupted = true;
            return StepPoll::Interrupt;
        }
        if elapsed < self.timeout + self.grace {
            return StepPoll::Wait;
        }
        StepPoll::Abandon
    }
}
