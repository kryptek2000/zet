//! Bounded, evidence-based observation of AI coding agents operating
//! inside ZET.
//!
//! This crate is the P4-A1 foundation. It deliberately stores *typed
//! evidence*, not conclusions: a known agent CLI being launched is a
//! heuristic candidate, never proof that the agent is active, successful,
//! authorized, or modifying files. Ordinary commands do not become agent
//! events.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Stable local identity for one observation. Monotonic within a store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ObservationId(pub u64);

/// Reference to the terminal session an observation belongs to. This is
/// the same opaque session identity recorded by `zet-blocks`; this crate
/// does not resolve or validate it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionRef(pub String);

/// Optional reference to a genuinely recorded command block. `None` means
/// the observation has no recorded command behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRef(pub i64);

/// Agent kinds ZET can name from a launch command. Unknown strings are
/// retained verbatim but are never treated as verified agent activity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentKind {
    OpenCode,
    GeminiCli,
    ClaudeCode,
    CodexCli,
    Aider,
    Cursor,
    Continue,
    Cline,
    Copilot,
    Goose,
    Unknown(String),
}

impl AgentKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::OpenCode => "opencode",
            Self::GeminiCli => "gemini",
            Self::ClaudeCode => "claude",
            Self::CodexCli => "codex",
            Self::Aider => "aider",
            Self::Cursor => "cursor",
            Self::Continue => "continue",
            Self::Cline => "cline",
            Self::Copilot => "copilot",
            Self::Goose => "goose",
            Self::Unknown(name) => name.as_str(),
        }
    }
}

/// Where an observation came from. Every observation has exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceSource {
    /// A command recorded by ZET's block store.
    RecordedCommand,
    /// Operating-system process table evidence. P4-A1 does not collect
    /// this; the variant exists so future collectors have a typed slot.
    ProcessTable,
    /// An explicit, consented structured integration. P4-A1 does not
    /// implement one.
    StructuredIntegration,
}

/// What kind of evidence was observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceKind {
    /// A known agent CLI was launched. Candidate only.
    CommandLaunchCandidate,
    /// A process believed to be an agent was observed.
    ProcessPresent,
    /// A structured event reported by an integration.
    StructuredEvent,
}

/// Fact vs. heuristic vs. unknown. This is intentionally separate from
/// `EvidenceKind`: a launch command is observed text, but the conclusion
/// "an agent is running" remains a heuristic candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObservationClass {
    ObservedFact,
    HeuristicCandidate,
    Unknown,
}

/// Exact evidence strength. There is no percentage confidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceStrength {
    Verified,
    Corroborated,
    Candidate,
    Unknown,
}

/// Availability/staleness of an observation or evidence stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceAvailability {
    Available,
    Stale,
    Unavailable,
    Conflicting,
}

/// One bounded, typed observation record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentObservation {
    pub id: ObservationId,
    pub session: SessionRef,
    pub source: EvidenceSource,
    pub evidence_kind: EvidenceKind,
    pub classification: ObservationClass,
    pub strength: EvidenceStrength,
    pub availability: EvidenceAvailability,
    pub agent: AgentKind,
    /// Recorded command block id, when the observation is tied to one.
    pub command: Option<CommandRef>,
    /// Bounded command name only. Full command text and output are never
    /// retained by this crate.
    pub command_name: Option<String>,
    pub observed_at_ms: u64,
    /// Age in milliseconds after which the observation is considered stale.
    pub stale_after_ms: u64,
}

impl AgentObservation {
    pub fn is_stale(&self, now_ms: u64) -> bool {
        now_ms >= self.observed_at_ms.saturating_add(self.stale_after_ms)
    }

    pub fn effective_availability(&self, now_ms: u64) -> EvidenceAvailability {
        if self.availability == EvidenceAvailability::Conflicting {
            return EvidenceAvailability::Conflicting;
        }
        if self.is_stale(now_ms) {
            return EvidenceAvailability::Stale;
        }
        self.availability
    }
}

/// Input for recording an observation from a future evidence collector.
/// `command_name` is truncated by the store.
#[derive(Debug, Clone)]
pub struct ObservationDraft {
    pub session: SessionRef,
    pub source: EvidenceSource,
    pub evidence_kind: EvidenceKind,
    pub classification: ObservationClass,
    pub strength: EvidenceStrength,
    pub agent: AgentKind,
    pub command: Option<CommandRef>,
    pub command_name: Option<String>,
    pub observed_at_ms: u64,
    pub stale_after_ms: u64,
    pub availability: EvidenceAvailability,
}

/// Read-only snapshot of the bounded observation history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentObservationSnapshot {
    pub observations: Vec<AgentObservation>,
    pub capacity: usize,
    pub len: usize,
    pub dropped: u64,
    pub process_evidence_available: bool,
    pub structured_events_available: bool,
    pub conflicting_sessions: Vec<String>,
}

/// Bounded in-memory history of agent observations.
#[derive(Debug)]
pub struct AgentObservationStore {
    capacity: usize,
    next_id: u64,
    observations: VecDeque<AgentObservation>,
    dropped: u64,
}

impl AgentObservationStore {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            next_id: 1,
            observations: VecDeque::new(),
            dropped: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.observations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.observations.is_empty()
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Record a fully-typed observation. The store assigns the id and
    /// enforces the bounded history. It does not validate the truth of
    /// the evidence; callers are responsible for only recording evidence
    /// they actually have.
    pub fn record(&mut self, draft: ObservationDraft) -> AgentObservation {
        let id = ObservationId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        let observation = AgentObservation {
            id,
            session: draft.session,
            source: draft.source,
            evidence_kind: draft.evidence_kind,
            classification: draft.classification,
            strength: draft.strength,
            availability: draft.availability,
            agent: draft.agent,
            command: draft.command,
            command_name: draft.command_name.map(|name| {
                let mut out: String = name.chars().take(64).collect();
                // Keep only the first token; never retain arguments.
                if let Some(idx) = out.find(char::is_whitespace) {
                    out.truncate(idx);
                }
                out
            }),
            observed_at_ms: draft.observed_at_ms,
            stale_after_ms: draft.stale_after_ms,
        };
        if self.observations.len() >= self.capacity {
            self.observations.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.observations.push_back(observation.clone());
        observation
    }

    /// Recognize a known agent CLI launch command as a *candidate* only.
    /// An ordinary or unknown command yields `None`.
    pub fn observe_command(
        &mut self,
        session: SessionRef,
        command_line: &str,
        command_id: Option<CommandRef>,
        now_ms: u64,
    ) -> Option<AgentObservation> {
        let name = first_command_token(command_line)?;
        let agent = known_agent(&name)?;
        let draft = ObservationDraft {
            session,
            source: EvidenceSource::RecordedCommand,
            evidence_kind: EvidenceKind::CommandLaunchCandidate,
            classification: ObservationClass::HeuristicCandidate,
            strength: EvidenceStrength::Candidate,
            agent,
            command: command_id,
            command_name: Some(name),
            observed_at_ms: now_ms,
            stale_after_ms: 300_000,
            availability: EvidenceAvailability::Available,
        };
        Some(self.record(draft))
    }

    pub fn observations(&self) -> impl Iterator<Item = &AgentObservation> {
        self.observations.iter()
    }

    pub fn for_session(&self, session: &SessionRef) -> Vec<AgentObservation> {
        self.observations
            .iter()
            .filter(|o| o.session == *session)
            .cloned()
            .collect()
    }

    pub fn snapshot(&self, _now_ms: u64) -> AgentObservationSnapshot {
        let observations: Vec<AgentObservation> = self.observations.iter().cloned().collect();
        let mut by_session: HashMap<String, Vec<AgentKind>> = HashMap::new();
        for obs in &observations {
            by_session
                .entry(obs.session.0.clone())
                .or_default()
                .push(obs.agent.clone());
        }
        let conflicting_sessions = by_session
            .into_iter()
            .filter(|(_, agents)| {
                let first = agents.first();
                agents.iter().any(|a| Some(a) != first)
            })
            .map(|(session, _)| session)
            .collect();
        AgentObservationSnapshot {
            observations,
            capacity: self.capacity,
            len: self.observations.len(),
            dropped: self.dropped,
            process_evidence_available: false,
            structured_events_available: false,
            conflicting_sessions,
        }
    }

    pub fn clear(&mut self) {
        self.observations.clear();
        self.dropped = 0;
    }

    pub fn clear_session(&mut self, session: &SessionRef) {
        self.observations.retain(|o| o.session != *session);
    }
}

fn first_command_token(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Skip leading VAR=value assignments.
    let mut rest = trimmed;
    loop {
        let token = rest.split_whitespace().next()?;
        if let Some(eq) = token.find('=') {
            let name = &token[..eq];
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                rest = rest[token.len()..].trim_start();
                if rest.is_empty() {
                    return None;
                }
                continue;
            }
        }
        let token = token.trim_matches(|c| c == '"' || c == '\'');
        if token.is_empty() {
            return None;
        }
        return Some(token.to_string());
    }
}

fn known_agent(name: &str) -> Option<AgentKind> {
    let base = name.rsplit('/').next().unwrap_or(name).to_lowercase();
    let base = base.trim_start_matches("./");
    match base {
        "opencode" => Some(AgentKind::OpenCode),
        "gemini" => Some(AgentKind::GeminiCli),
        "claude" => Some(AgentKind::ClaudeCode),
        "codex" => Some(AgentKind::CodexCli),
        "aider" => Some(AgentKind::Aider),
        "cursor" => Some(AgentKind::Cursor),
        "continue" | "cn" => Some(AgentKind::Continue),
        "cline" => Some(AgentKind::Cline),
        "copilot" => Some(AgentKind::Copilot),
        "goose" => Some(AgentKind::Goose),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_command_does_not_become_observation() {
        let mut store = AgentObservationStore::new(8);
        assert!(store
            .observe_command(SessionRef("s1".into()), "cargo test", None, 1000)
            .is_none());
        assert!(store.is_empty());
    }

    #[test]
    fn known_agent_command_is_candidate_only() {
        let mut store = AgentObservationStore::new(8);
        let obs = store
            .observe_command(SessionRef("s1".into()), "opencode run foo", None, 1000)
            .expect("candidate");
        assert_eq!(obs.classification, ObservationClass::HeuristicCandidate);
        assert_eq!(obs.strength, EvidenceStrength::Candidate);
        assert_eq!(obs.evidence_kind, EvidenceKind::CommandLaunchCandidate);
        assert_eq!(obs.agent, AgentKind::OpenCode);
        assert_eq!(obs.availability, EvidenceAvailability::Available);
        assert!(!obs.is_stale(1000));
    }

    #[test]
    fn unknown_agent_cli_is_not_observed() {
        let mut store = AgentObservationStore::new(8);
        assert!(store
            .observe_command(SessionRef("s1".into()), "my-agent-cli do work", None, 1000)
            .is_none());
    }

    #[test]
    fn multiple_sessions_stay_separate() {
        let mut store = AgentObservationStore::new(8);
        store.observe_command(SessionRef("s1".into()), "claude", Some(CommandRef(1)), 1000);
        store.observe_command(SessionRef("s2".into()), "codex", Some(CommandRef(2)), 1000);
        assert_eq!(store.for_session(&SessionRef("s1".into())).len(), 1);
        assert_eq!(store.for_session(&SessionRef("s2".into())).len(), 1);
        assert_eq!(store.for_session(&SessionRef("s3".into())).len(), 0);
    }

    #[test]
    fn bounded_history_drops_oldest() {
        let mut store = AgentObservationStore::new(2);
        store.observe_command(SessionRef("s".into()), "opencode", None, 1);
        store.observe_command(SessionRef("s".into()), "claude", None, 2);
        store.observe_command(SessionRef("s".into()), "codex", None, 3);
        assert_eq!(store.len(), 2);
        assert_eq!(store.dropped(), 1);
        let ids: Vec<_> = store.observations().map(|o| o.id).collect();
        assert!(ids[0] < ids[1]);
    }

    #[test]
    fn stale_and_conflicting_evidence_are_reported() {
        let mut store = AgentObservationStore::new(8);
        store.observe_command(SessionRef("s".into()), "opencode", None, 1000);
        store.observe_command(SessionRef("s".into()), "claude", None, 1001);
        let snap = store.snapshot(1000 + 400_000);
        assert!(snap
            .observations
            .iter()
            .all(|o| o.effective_availability(1000 + 400_000) == EvidenceAvailability::Stale));
        assert!(snap.conflicting_sessions.contains(&"s".to_string()));
        assert!(!snap.process_evidence_available);
        assert!(!snap.structured_events_available);
    }

    #[test]
    fn clear_session_removes_only_that_session() {
        let mut store = AgentObservationStore::new(8);
        store.observe_command(SessionRef("s1".into()), "opencode", None, 1);
        store.observe_command(SessionRef("s2".into()), "claude", None, 2);
        store.clear_session(&SessionRef("s1".into()));
        assert!(store.for_session(&SessionRef("s1".into())).is_empty());
        assert_eq!(store.for_session(&SessionRef("s2".into())).len(), 1);
    }

    #[test]
    fn restart_session_identity_does_not_carry_over() {
        // A new session id simply has no observations.
        let store = AgentObservationStore::new(8);
        assert!(store
            .for_session(&SessionRef("new-session".into()))
            .is_empty());
    }

    #[test]
    fn command_name_is_bounded_and_argument_free() {
        let mut store = AgentObservationStore::new(8);
        let obs = store
            .observe_command(
                SessionRef("s".into()),
                "opencode --secret-token=abc run",
                None,
                1,
            )
            .unwrap();
        assert_eq!(obs.command_name.as_deref(), Some("opencode"));
    }

    #[test]
    fn record_preserves_explicit_unknown_state() {
        let mut store = AgentObservationStore::new(4);
        let obs = store.record(ObservationDraft {
            session: SessionRef("s".into()),
            source: EvidenceSource::ProcessTable,
            evidence_kind: EvidenceKind::ProcessPresent,
            classification: ObservationClass::Unknown,
            strength: EvidenceStrength::Unknown,
            agent: AgentKind::Unknown("mystery".into()),
            command: None,
            command_name: None,
            observed_at_ms: 5,
            stale_after_ms: 10,
            availability: EvidenceAvailability::Unavailable,
        });
        assert_eq!(obs.classification, ObservationClass::Unknown);
        assert_eq!(obs.availability, EvidenceAvailability::Unavailable);
    }
}
