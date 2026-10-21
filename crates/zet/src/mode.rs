//! Which mode `zet` runs in: the CLI, the window, or usage.
//!
//! The rule table is frozen (docs/design/A1-binary-window-unification.md
//! §5): first match wins. A known subcommand is the CLI; a bare unknown
//! word is a typo (usage, exit 2); a phrase is a goal (window). The
//! table lives here — pure, no GUI, no IO — so it can be unit-tested
//! without a display.

/// What `zet` was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// A CLI subcommand (or a help/version flag): run the CLI.
    Cli,
    /// Open the window. `goal` is the turn to start once the shell is up.
    Window { goal: Option<String> },
    /// Nothing to run: print the usage and exit 2.
    Usage,
}

/// What this process can see: a display, and whether a window is allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Env {
    /// A display server is reachable (X11 or Wayland on Linux).
    pub display: bool,
    /// Nothing vetoed the window (`ZET_NO_WINDOW` unset).
    pub window_allowed: bool,
}

impl Env {
    /// Read the environment: display reachability and the window veto.
    pub fn detect() -> Self {
        Env {
            display: display_available(),
            window_allowed: std::env::var_os("ZET_NO_WINDOW").is_none(),
        }
    }

    /// A window can actually be opened here: a display is reachable and
    /// nothing vetoed it.
    pub fn can_open_window(&self) -> bool {
        self.display && self.window_allowed
    }
}

/// A display server we can open a window on. Linux needs X11 or Wayland;
/// macOS and Windows always have one.
#[cfg(target_os = "linux")]
fn display_available() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some()
}

#[cfg(not(target_os = "linux"))]
fn display_available() -> bool {
    true
}

/// The goal text for a window run: the argv tail joined with spaces.
/// `None` when there is nothing to say.
fn goal_of(args: &[String]) -> Option<String> {
    let joined = args.join(" ");
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

/// What the argv asks for, before the environment has its say. One
/// function, so the rule table cannot drift between the planner and
/// the refusal check.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Request {
    /// A CLI subcommand or a help/version flag.
    Cli,
    /// A window, with the goal to start once the shell is up.
    Window { goal: Option<String> },
    /// Nothing to run: a typo.
    Usage,
}

/// Read the argv. First match wins (the frozen rule table):
///
/// 1. `--help`/`-h`/`--version`/`-V` — the CLI.
/// 2. `--window`/`--gui` — a window, goal = the rest.
/// 3. A known CLI group — the CLI.
/// 4. An internal `__` arm (`__run_worker`) — the CLI.
/// 5. No args — a window (no goal).
/// 6. Several words, or one word with spaces in it — a window with that goal.
/// 7. One unknown word — a typo: usage.
fn request(args: &[String]) -> Request {
    let Some(first) = args.first().map(String::as_str) else {
        // Rule 5: bare `zet` opens the window.
        return Request::Window { goal: None };
    };
    match first {
        // Rule 1: help and version stay on the CLI (today: usage, exit 2).
        "--help" | "-h" | "--version" | "-V" => Request::Cli,
        // Rule 2: the explicit window flag — the escape hatch for a
        // one-word goal. It asks for a window, so it ignores rule 7.
        "--window" | "--gui" => Request::Window {
            goal: goal_of(&args[1..]),
        },
        // Rule 3: a known CLI group. The list is shared with the window
        // router so the two can never drift apart.
        sub if zet_pty::CLI_SUBCOMMANDS.contains(&sub) => Request::Cli,
        // Rule 4: internal arms (`__run_worker`) are CLI, never a window.
        sub if sub.starts_with("__") => Request::Cli,
        // Rule 6: more than one word, or one word that carries spaces —
        // that is a goal, not a typo.
        first if args.len() >= 2 || first.contains(char::is_whitespace) => Request::Window {
            goal: goal_of(args),
        },
        // Rule 7: a single unknown word is a typo. Usage, exit 2.
        _ => Request::Usage,
    }
}

/// Decide the mode from argv and the environment. First match wins:
/// help/version flags, the window flags, a known subcommand, an internal
/// `__` arm, bare invocation, a goal, and finally usage.
pub fn plan(args: &[String], env: &Env) -> Mode {
    match request(args) {
        Request::Cli => Mode::Cli,
        Request::Usage => Mode::Usage,
        Request::Window { goal } => {
            // A window is only opened where one can be opened: a display
            // must be there and nothing must have vetoed it. Without
            // that, the run is a usage error — never a panic, never a
            // window that opens nowhere.
            if env.can_open_window() {
                Mode::Window { goal }
            } else {
                Mode::Usage
            }
        }
    }
}

/// Did the argv ask for a window that this environment cannot open? No
/// display, or the `ZET_NO_WINDOW` veto. The caller says so plainly
/// instead of printing a bare usage.
pub fn window_refused(args: &[String], env: &Env) -> bool {
    matches!(request(args), Request::Window { .. }) && !env.can_open_window()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(display: bool) -> Env {
        Env {
            display,
            window_allowed: true,
        }
    }

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    /// Rule 1: help and version flags are CLI (they print usage, exit 2).
    #[test]
    fn rule1_help_and_version_flags_are_cli() {
        for flag in ["--help", "-h", "--version", "-V"] {
            assert_eq!(plan(&argv(&[flag]), &env(true)), Mode::Cli, "{flag}");
        }
    }

    /// Rule 2: the window flag asks for a window, with the tail as goal.
    #[test]
    fn rule2_window_flag_asks_for_the_window() {
        assert_eq!(
            plan(&argv(&["--window"]), &env(true)),
            Mode::Window { goal: None }
        );
        assert_eq!(
            plan(&argv(&["--window", "hello"]), &env(true)),
            Mode::Window {
                goal: Some("hello".to_string())
            }
        );
        assert_eq!(
            plan(&argv(&["--gui", "fix", "the", "test"]), &env(true)),
            Mode::Window {
                goal: Some("fix the test".to_string())
            }
        );
    }

    /// Rule 3: every known CLI group stays on the CLI path.
    #[test]
    fn rule3_known_subcommands_are_cli() {
        for group in zet_pty::CLI_SUBCOMMANDS {
            assert_eq!(plan(&argv(&[group]), &env(true)), Mode::Cli, "{group}");
        }
    }

    /// Rule 4: the daemon's internal arm is a CLI arm, not a goal.
    #[test]
    fn rule4_internal_arms_are_cli() {
        for arm in ["__run_worker", "__anything"] {
            assert_eq!(plan(&argv(&[arm, "id-1"]), &env(true)), Mode::Cli, "{arm}");
        }
    }

    /// Rule 5: bare `zet` opens the window when a display is there.
    #[test]
    fn rule5_bare_invocation_opens_the_window() {
        assert_eq!(plan(&argv(&[]), &env(true)), Mode::Window { goal: None });
    }

    /// Rule 5: headless, bare `zet` is a usage error — never a panic.
    #[test]
    fn rule5_bare_invocation_without_a_display_is_usage() {
        assert_eq!(plan(&argv(&[]), &env(false)), Mode::Usage);
    }

    /// Rule 6: a quoted phrase (one argv element with spaces) and a
    /// multi-word goal both open the window with the goal attached.
    #[test]
    fn rule6_phrases_are_goals() {
        assert_eq!(
            plan(&argv(&["fix failing e2e in ./api"]), &env(true)),
            Mode::Window {
                goal: Some("fix failing e2e in ./api".to_string())
            }
        );
        assert_eq!(
            plan(&argv(&["fix", "the", "failing", "test"]), &env(true)),
            Mode::Window {
                goal: Some("fix the failing test".to_string())
            }
        );
    }

    /// Rule 6: a goal with no display is a usage error, not a panic.
    #[test]
    fn rule6_goal_without_a_display_is_usage() {
        assert_eq!(
            plan(&argv(&["fix", "the", "test"]), &env(false)),
            Mode::Usage
        );
    }

    /// Rule 7: a single unknown word is a typo, not a goal.
    #[test]
    fn rule7_unknown_single_word_is_usage() {
        assert_eq!(plan(&argv(&["bogus"]), &env(true)), Mode::Usage);
    }

    /// The window veto (`ZET_NO_WINDOW`) blocks the window too — the
    /// explicit flag included.
    #[test]
    fn the_window_veto_blocks_every_window() {
        let vetoed = Env {
            display: true,
            window_allowed: false,
        };
        assert_eq!(plan(&argv(&[]), &vetoed), Mode::Usage);
        assert_eq!(plan(&argv(&["fix", "the", "test"]), &vetoed), Mode::Usage);
        assert_eq!(plan(&argv(&["--window"]), &vetoed), Mode::Usage);
    }

    /// A refused window is reported as such: the argv asked for a window
    /// the environment cannot open. A typo is not a refusal.
    #[test]
    fn a_refused_window_is_reported() {
        assert!(window_refused(&argv(&[]), &env(false)));
        assert!(window_refused(&argv(&["fix", "the", "test"]), &env(false)));
        assert!(window_refused(&argv(&["--window"]), &env(false)));
        // Nothing was refused: the argv was a typo, not a window.
        assert!(!window_refused(&argv(&["bogus"]), &env(false)));
        // The window opens fine.
        assert!(!window_refused(&argv(&[]), &env(true)));
    }

    /// The dispatch list and the shared list must agree: every group in
    /// `zet_pty::CLI_SUBCOMMANDS` has to reach the CLI, whatever the
    /// environment says.
    #[test]
    fn cli_group_list_and_mode_table_agree() {
        for group in zet_pty::CLI_SUBCOMMANDS {
            assert_eq!(plan(&argv(&[group]), &env(false)), Mode::Cli, "{group}");
            assert_eq!(
                plan(&argv(&[group, "--definitely-bogus"]), &env(false)),
                Mode::Cli,
                "{group}"
            );
        }
    }
}
