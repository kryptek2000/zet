use std::time::Duration;
use zet_gpu::exec_watch::{StepPoll, StepWatch};

#[test]
fn step_settles_fast_no_interrupt() {
    let mut w = StepWatch::new(Duration::from_secs(120), Duration::from_secs(10));
    assert!(matches!(
        w.poll(Duration::from_secs(3), true),
        StepPoll::Settle
    ));
    assert!(!w.interrupted());
}

#[test]
fn silent_step_gets_one_interrupt_then_abandon() {
    let mut w = StepWatch::new(Duration::from_secs(120), Duration::from_secs(10));
    assert!(matches!(
        w.poll(Duration::from_secs(119), false),
        StepPoll::Wait
    ));
    // Deadline with nothing recorded: interrupt once (^C reclaims stdin
    // waits like bat-on-stdin; the 130 block usually settles after).
    assert!(matches!(
        w.poll(Duration::from_secs(120), false),
        StepPoll::Interrupt
    ));
    assert!(w.interrupted());
    // Grace window: still waiting, no second interrupt.
    assert!(matches!(
        w.poll(Duration::from_secs(125), false),
        StepPoll::Wait
    ));
    assert!(matches!(
        w.poll(Duration::from_secs(125), false),
        StepPoll::Wait
    ));
    // Prompt never returned: abandon so the loop survives.
    assert!(matches!(
        w.poll(Duration::from_secs(131), false),
        StepPoll::Abandon
    ));
}

#[test]
fn late_settle_after_interrupt_still_counts() {
    let mut w = StepWatch::new(Duration::from_secs(120), Duration::from_secs(10));
    assert!(matches!(
        w.poll(Duration::from_secs(120), false),
        StepPoll::Interrupt
    ));
    // The ^C-killed block settled during grace: normal completion.
    assert!(matches!(
        w.poll(Duration::from_secs(124), true),
        StepPoll::Settle
    ));
}
