//! Bounded PTY shutdown drain.
//!
//! Closing a tab or the window must not synchronously collect every queued
//! PTY chunk: a busy command can make the channel arbitrarily deep, and the
//! UI thread cannot afford to parse it all before the window closes. The
//! shutdown path therefore feeds a bounded number of bytes per pass through
//! the real terminal parser, preserving receive/marker order.
//!
//! The producer must be stopped (child reaped) before this drain is used to
//! decide whether delivery is complete. Only a disconnected channel proves
//! that no more bytes can arrive; an empty `try_recv()` while the sender is
//! alive is reported as [`DrainOutcome::LiveEmpty`], never as completion.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use zet_term::Terminal;

/// Chunk cap for one shutdown pass. The production reader sends 4 KiB
/// chunks, so this is at most 256 KiB of parser work per pass.
pub(crate) const FINAL_DRAIN_MAX_CHUNKS: usize = 64;
/// Byte cap for one shutdown pass. Bounds parser work independently of how
/// the channel was fed (tests and future transports may use larger chunks).
pub(crate) const FINAL_DRAIN_MAX_BYTES: usize = 256 * 1024;
/// Wall-clock cap for one pass. Closing may wait briefly for the reader
/// thread to see EOF after the child is reaped, but it must never freeze
/// the UI.
pub(crate) const FINAL_DRAIN_MAX_DURATION: Duration = Duration::from_millis(25);
/// Total shutdown drain budget. A pass stops at the chunk/byte/time caps;
/// the outer drain then starts another pass rather than discarding the
/// remainder merely because a per-pass limit was reached. 100 ms is below
/// the 250 ms idle frame, and only application/tab close pays it.
pub(crate) const FINAL_DRAIN_TOTAL_DURATION: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DrainBudget {
    pub max_chunks: usize,
    pub max_bytes: usize,
    pub max_duration: Duration,
}

pub(crate) const FINAL_DRAIN_BUDGET: DrainBudget = DrainBudget {
    max_chunks: FINAL_DRAIN_MAX_CHUNKS,
    max_bytes: FINAL_DRAIN_MAX_BYTES,
    max_duration: FINAL_DRAIN_MAX_DURATION,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrainOutcome {
    /// The sender was dropped: no more bytes can arrive.
    Disconnected,
    /// The channel is still connected but had no bytes before the deadline.
    LiveEmpty,
    /// The work budget ended with bytes still possible.
    BudgetExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DrainReport {
    pub chunks: usize,
    pub bytes: usize,
    pub outcome: DrainOutcome,
}

impl DrainReport {
    /// A tab with no receiver has no pending PTY delivery to lose.
    pub(crate) fn no_receiver() -> Self {
        Self {
            chunks: 0,
            bytes: 0,
            outcome: DrainOutcome::Disconnected,
        }
    }

    /// Complete only when the producer side is known to be gone.
    pub(crate) fn is_complete(self) -> bool {
        self.outcome == DrainOutcome::Disconnected
    }
}

/// Feed queued PTY chunks through `term` until the producer disconnects or
/// the total shutdown budget is spent. Each pass is independently bounded;
/// when a pass ends on its chunk/byte cap, the remaining queued bytes are
/// handled by the next pass instead of being discarded. Returns the work
/// performed so callers can distinguish "producer finished" from "receiver
/// was merely idle".
pub(crate) fn drain_receiver_bounded(rx: &Receiver<Vec<u8>>, term: &mut Terminal) -> DrainReport {
    let start = Instant::now();
    let deadline = start
        .checked_add(FINAL_DRAIN_TOTAL_DURATION)
        .unwrap_or(start);
    let mut chunks = 0usize;
    let mut bytes = 0usize;

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return DrainReport {
                chunks,
                bytes,
                outcome: DrainOutcome::BudgetExhausted,
            };
        }
        let pass_budget = DrainBudget {
            max_duration: FINAL_DRAIN_BUDGET.max_duration.min(remaining),
            ..FINAL_DRAIN_BUDGET
        };
        let pass = drain_with_budget(rx, term, pass_budget);
        chunks = chunks.saturating_add(pass.chunks);
        bytes = bytes.saturating_add(pass.bytes);
        match pass.outcome {
            DrainOutcome::Disconnected => {
                return DrainReport {
                    chunks,
                    bytes,
                    outcome: DrainOutcome::Disconnected,
                };
            }
            DrainOutcome::LiveEmpty | DrainOutcome::BudgetExhausted => {
                if Instant::now() >= deadline {
                    return DrainReport {
                        chunks,
                        bytes,
                        outcome: pass.outcome,
                    };
                }
            }
        }
    }
}

pub(crate) fn drain_with_budget(
    rx: &Receiver<Vec<u8>>,
    term: &mut Terminal,
    budget: DrainBudget,
) -> DrainReport {
    let start = Instant::now();
    let deadline = start.checked_add(budget.max_duration).unwrap_or(start);
    let mut chunks = 0usize;
    let mut bytes = 0usize;

    loop {
        if chunks >= budget.max_chunks || bytes >= budget.max_bytes {
            return probe_after_budget(rx, term, chunks, bytes);
        }
        if Instant::now() >= deadline {
            return probe_after_budget(rx, term, chunks, bytes);
        }
        match rx.try_recv() {
            Ok(chunk) => {
                bytes = bytes.saturating_add(chunk.len());
                chunks = chunks.saturating_add(1);
                term.feed(&chunk);
            }
            Err(TryRecvError::Empty) => {
                // Empty is not EOF: the producer may still write. Give the
                // reader thread a bounded moment to observe child exit and
                // drop its sender, then report the channel as still live.
                if Instant::now() >= deadline {
                    return DrainReport {
                        chunks,
                        bytes,
                        outcome: DrainOutcome::LiveEmpty,
                    };
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(TryRecvError::Disconnected) => {
                return DrainReport {
                    chunks,
                    bytes,
                    outcome: DrainOutcome::Disconnected,
                };
            }
        }
    }
}

/// The budget check happens before `try_recv`, so a byte that is already
/// queued would otherwise be left behind when the limit is reached. Probe
/// once: disconnect still means completion, empty means live, and a queued
/// chunk is fed rather than discarded (the cap is a soft bound: at most one
/// extra chunk). Anything still queued after that is reported as
/// [`DrainOutcome::BudgetExhausted`], never silently treated as complete.
fn probe_after_budget(
    rx: &Receiver<Vec<u8>>,
    term: &mut Terminal,
    mut chunks: usize,
    mut bytes: usize,
) -> DrainReport {
    match rx.try_recv() {
        Err(TryRecvError::Disconnected) => DrainReport {
            chunks,
            bytes,
            outcome: DrainOutcome::Disconnected,
        },
        Err(TryRecvError::Empty) => DrainReport {
            chunks,
            bytes,
            outcome: DrainOutcome::LiveEmpty,
        },
        Ok(chunk) => {
            bytes = bytes.saturating_add(chunk.len());
            chunks = chunks.saturating_add(1);
            term.feed(&chunk);
            DrainReport {
                chunks,
                bytes,
                outcome: DrainOutcome::BudgetExhausted,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{self, Sender};

    fn small_budget() -> DrainBudget {
        DrainBudget {
            max_chunks: 4,
            max_bytes: 1024,
            max_duration: Duration::from_secs(1),
        }
    }

    #[test]
    fn empty_live_receiver_is_not_complete() {
        let (_tx, rx) = mpsc::channel::<Vec<u8>>();
        let mut term = Terminal::new(40, 10);
        let report = drain_with_budget(&rx, &mut term, small_budget());
        assert_eq!(report.outcome, DrainOutcome::LiveEmpty);
        assert!(!report.is_complete());
        assert_eq!(report.chunks, 0);
        assert_eq!(report.bytes, 0);
    }

    #[test]
    fn disconnect_is_the_only_completion_evidence() {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        tx.send(b"late".to_vec()).unwrap();
        drop(tx);
        let mut term = Terminal::new(40, 10);
        let report = drain_with_budget(&rx, &mut term, small_budget());
        assert_eq!(report.outcome, DrainOutcome::Disconnected);
        assert!(report.is_complete());
        assert_eq!(report.chunks, 1);
        assert_eq!(report.bytes, 4);
        assert_eq!(term.line_text(0).trim_end(), "late");
    }

    #[test]
    fn chunk_budget_leaves_later_chunks_in_the_channel() {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        send_chunks(&tx, 20, b"01234567");
        let mut term = Terminal::new(40, 10);
        let report = drain_with_budget(&rx, &mut term, small_budget());

        // Four budgeted chunks plus the one probed chunk, never all twenty.
        assert_eq!(report.outcome, DrainOutcome::BudgetExhausted);
        assert!(!report.is_complete());
        assert_eq!(report.chunks, 5);
        assert_eq!(report.bytes, 40);
        assert!(report.chunks <= small_budget().max_chunks + 1);
        assert!(report.bytes <= small_budget().max_bytes);

        // The helper did not collect the rest into a Vec: they are still
        // queued for the next bounded pass (or for the tab to drop).
        let mut remaining = 0;
        while rx.try_recv().is_ok() {
            remaining += 1;
        }
        assert_eq!(remaining, 15);
    }

    #[test]
    fn late_bytes_keep_parser_order_across_passes() {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let mut term = Terminal::new(40, 10);
        tx.send(b"$ \x1b]133;B\x07".to_vec()).unwrap();

        let first = drain_with_budget(&rx, &mut term, small_budget());
        assert_eq!(first.outcome, DrainOutcome::LiveEmpty);
        assert!(!first.is_complete());

        // The producer emits the rest only after the first pass gave up.
        tx.send(b"echo late\x1b]133;C\x07\r\nlate\r\n\x1b]133;D;0\x07".to_vec())
            .unwrap();
        drop(tx);
        let second = drain_with_budget(&rx, &mut term, small_budget());
        assert_eq!(second.outcome, DrainOutcome::Disconnected);
        assert!(second.is_complete());

        let markers = term.drain_markers();
        let kinds: Vec<_> = markers.iter().map(|m| m.kind).collect();
        assert_eq!(
            kinds,
            vec![
                zet_term::MarkerKind::Input,
                zet_term::MarkerKind::Exec,
                zet_term::MarkerKind::Done,
            ]
        );
    }

    #[test]
    fn multi_pass_drain_does_not_stop_at_a_chunk_limit() {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        send_chunks(&tx, 200, b"01234567");
        drop(tx);
        let mut term = Terminal::new(40, 10);

        // The per-pass cap is 64 chunks, but the shutdown drain keeps going
        // until the producer disconnects: 200 queued chunks are not thrown
        // away merely because one pass reached its limit.
        let report = drain_receiver_bounded(&rx, &mut term);
        assert_eq!(report.outcome, DrainOutcome::Disconnected);
        assert!(report.is_complete());
        assert_eq!(report.chunks, 200);
        assert_eq!(report.bytes, 200 * 8);
    }

    fn send_chunks(tx: &Sender<Vec<u8>>, count: usize, payload: &[u8]) {
        for _ in 0..count {
            tx.send(payload.to_vec()).unwrap();
        }
    }
}
