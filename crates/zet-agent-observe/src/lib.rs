//! Bounded, evidence-based observation of AI coding agents operating
//! inside ZET.
//!
//! P4-A1 foundation. A known agent CLI launch is a heuristic candidate,
//! never proof of active execution, success, authorization, or file changes.
//! Ordinary commands do not become agent observations.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Absolute product-level bound for retained observations.
pub const MAX_OBSERVATION_CAPACITY: usize = 1024;
/// Absolute byte bound for session references.
pub const MAX_SESSION_REF_BYTES: usize = 128;
/// Absolute byte bound for unknown agent names.
pub const MAX_AGENT_NAME_BYTES: usize = 64;
/// Absolute byte bound for retained command names.
pub const MAX_COMMAND_NAME_BYTES: usize = 64;
/// Absolute bounds for a candidate's staleness window.
pub const MIN_STALE_AFTER_MS: u64 = 1_000;
pub const MAX_STALE_AFTER_MS: u64 = 86_400_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ObservationId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct SessionRef(String);

impl SessionRef {
    pub fn new(value: impl Into<String>) -> Result<Self, ObservationError> {
        let value = value.into();
        if value.len() > MAX_SESSION_REF_BYTES {
            return Err(ObservationError::OversizedInput);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> serde::Deserialize<'de> for SessionRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        SessionRef::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CommandRef(pub(crate) i64);

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

    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceSource {
    /// A command recorded by ZET's block store, with a validated block id.
    RecordedCommand,
    /// Recognized command text that is not yet tied to a persisted block.
    CommandText,
    /// Process-table evidence. Not collected in P4-A1.
    ProcessTable,
    /// Authenticated structured integration. Not implemented in P4-A1.
    StructuredIntegration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceKind {
    CommandLaunchCandidate,
    ProcessPresent,
    StructuredEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObservationClass {
    ObservedFact,
    HeuristicCandidate,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceStrength {
    Verified,
    Corroborated,
    Candidate,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceAvailability {
    Available,
    Stale,
    Unavailable,
    Conflicting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictStatus {
    /// P4-A1 cannot establish a real conflict from available evidence.
    Unknown,
    None,
    Conflicting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentObservation {
    id: ObservationId,
    session: SessionRef,
    source: EvidenceSource,
    evidence_kind: EvidenceKind,
    classification: ObservationClass,
    strength: EvidenceStrength,
    availability: EvidenceAvailability,
    agent: AgentKind,
    command: Option<CommandRef>,
    command_name: Option<String>,
    observed_at_ms: u64,
    stale_after_ms: u64,
}

/// Untrusted serialized representation of an observation claim.
/// This is a data DTO only; it does not establish trusted authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentObservationDto {
    pub id: u64,
    pub session: String,
    pub source: EvidenceSource,
    pub evidence_kind: EvidenceKind,
    pub classification: ObservationClass,
    pub strength: EvidenceStrength,
    pub availability: EvidenceAvailability,
    pub agent: AgentKind,
    pub command: Option<i64>,
    pub command_name: Option<String>,
    pub observed_at_ms: u64,
    pub stale_after_ms: u64,
}

impl AgentObservation {
    pub fn to_dto(&self) -> AgentObservationDto {
        AgentObservationDto {
            id: self.id.0,
            session: self.session.as_str().to_string(),
            source: self.source,
            evidence_kind: self.evidence_kind,
            classification: self.classification,
            strength: self.strength,
            availability: self.availability,
            agent: self.agent.clone(),
            command: self.command.map(|c| c.0),
            command_name: self.command_name.clone(),
            observed_at_ms: self.observed_at_ms,
            stale_after_ms: self.stale_after_ms,
        }
    }

    pub fn from_dto(
        dto: AgentObservationDto,
        store: Option<&zet_blocks::Store>,
    ) -> Result<Self, ObservationError> {
        let session = SessionRef::new(dto.session)?;
        if dto.stale_after_ms < MIN_STALE_AFTER_MS || dto.stale_after_ms > MAX_STALE_AFTER_MS {
            return Err(ObservationError::InvalidTimestamp);
        }
        if dto.command_name.as_ref().map(|s| s.len()).unwrap_or(0) > MAX_COMMAND_NAME_BYTES {
            return Err(ObservationError::OversizedInput);
        }
        match dto.source {
            EvidenceSource::RecordedCommand => {
                if dto.evidence_kind != EvidenceKind::CommandLaunchCandidate
                    || dto.classification != ObservationClass::HeuristicCandidate
                    || dto.strength != EvidenceStrength::Candidate
                    || dto.agent.is_unknown()
                {
                    return Err(ObservationError::InvalidEvidence);
                }
                let block_id = dto.command.ok_or(ObservationError::InvalidEvidence)?;
                let store = store.ok_or(ObservationError::InvalidEvidence)?;
                let block = store
                    .get(block_id)
                    .map_err(|_| ObservationError::InvalidEvidence)?;
                if block.session != session.as_str() {
                    return Err(ObservationError::InvalidEvidence);
                }
                Ok(AgentObservation {
                    id: ObservationId(dto.id),
                    session,
                    source: EvidenceSource::RecordedCommand,
                    evidence_kind: EvidenceKind::CommandLaunchCandidate,
                    classification: ObservationClass::HeuristicCandidate,
                    strength: EvidenceStrength::Candidate,
                    availability: dto.availability,
                    agent: dto.agent,
                    command: Some(CommandRef(block_id)),
                    command_name: dto.command_name,
                    observed_at_ms: dto.observed_at_ms,
                    stale_after_ms: dto.stale_after_ms,
                })
            }
            EvidenceSource::CommandText => {
                if dto.command.is_some() {
                    return Err(ObservationError::InvalidEvidence);
                }
                if dto.evidence_kind != EvidenceKind::CommandLaunchCandidate
                    || dto.classification != ObservationClass::HeuristicCandidate
                    || dto.strength != EvidenceStrength::Candidate
                    || dto.agent.is_unknown()
                {
                    return Err(ObservationError::InvalidEvidence);
                }
                Ok(AgentObservation {
                    id: ObservationId(dto.id),
                    session,
                    source: EvidenceSource::CommandText,
                    evidence_kind: EvidenceKind::CommandLaunchCandidate,
                    classification: ObservationClass::HeuristicCandidate,
                    strength: EvidenceStrength::Candidate,
                    availability: dto.availability,
                    agent: dto.agent,
                    command: None,
                    command_name: dto.command_name,
                    observed_at_ms: dto.observed_at_ms,
                    stale_after_ms: dto.stale_after_ms,
                })
            }
            EvidenceSource::ProcessTable | EvidenceSource::StructuredIntegration => {
                Err(ObservationError::UnsupportedSource)
            }
        }
    }

    pub fn id(&self) -> ObservationId {
        self.id
    }
    pub fn session(&self) -> &SessionRef {
        &self.session
    }
    pub fn source(&self) -> EvidenceSource {
        self.source
    }
    pub fn evidence_kind(&self) -> EvidenceKind {
        self.evidence_kind
    }
    pub fn classification(&self) -> ObservationClass {
        self.classification
    }
    pub fn strength(&self) -> EvidenceStrength {
        self.strength
    }
    pub fn availability(&self) -> EvidenceAvailability {
        self.availability
    }
    pub fn agent(&self) -> &AgentKind {
        &self.agent
    }
    pub fn command(&self) -> Option<CommandRef> {
        self.command
    }
    pub fn command_name(&self) -> Option<&str> {
        self.command_name.as_deref()
    }
    pub fn observed_at_ms(&self) -> u64 {
        self.observed_at_ms
    }
    pub fn stale_after_ms(&self) -> u64 {
        self.stale_after_ms
    }

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentObservationSnapshot {
    pub observations: Vec<AgentObservation>,
    pub capacity: usize,
    pub len: usize,
    pub dropped: u64,
    pub recorded_dropped: u64,
    pub recorded_loss_complete: bool,
    pub conflict_status: ConflictStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationError {
    UnsupportedSource,
    InvalidEvidence,
    OversizedInput,
    InvalidTimestamp,
    IdExhausted,
}

impl std::fmt::Display for ObservationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSource => write!(f, "unsupported evidence source"),
            Self::InvalidEvidence => write!(f, "invalid evidence combination"),
            Self::OversizedInput => write!(f, "input exceeds hard bound"),
            Self::InvalidTimestamp => write!(f, "invalid timestamp or staleness window"),
            Self::IdExhausted => write!(f, "observation id space exhausted"),
        }
    }
}

impl std::error::Error for ObservationError {}

#[derive(Debug)]
pub struct AgentObservationStore {
    capacity: usize,
    next_id: u64,
    observations: VecDeque<AgentObservation>,
    dropped: u64,
    recorded_dropped: u64,
    recorded_dropped_by_session: HashMap<String, u64>,
    recorded_loss_complete: bool,
    seen_recorded: std::collections::HashSet<(String, i64)>,
}

impl AgentObservationStore {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.clamp(1, MAX_OBSERVATION_CAPACITY);
        Self {
            capacity,
            next_id: 1,
            observations: VecDeque::new(),
            dropped: 0,
            recorded_dropped: 0,
            recorded_dropped_by_session: HashMap::new(),
            recorded_loss_complete: true,
            seen_recorded: std::collections::HashSet::new(),
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

    pub fn try_record(
        &mut self,
        draft: ObservationDraft,
    ) -> Result<AgentObservation, ObservationError> {
        validate_draft(&draft)?;
        let id = if self.next_id == u64::MAX {
            return Err(ObservationError::IdExhausted);
        } else {
            let id = self.next_id;
            self.next_id += 1;
            id
        };

        let session = if draft.session.as_str().len() > MAX_SESSION_REF_BYTES {
            return Err(ObservationError::OversizedInput);
        } else {
            draft.session
        };
        let agent = match draft.agent {
            AgentKind::Unknown(name) => {
                if name.len() > MAX_AGENT_NAME_BYTES {
                    return Err(ObservationError::OversizedInput);
                }
                AgentKind::Unknown(name)
            }
            other => other,
        };
        let command_name = draft.command_name.map(|name| {
            let mut out: String = name.chars().take(MAX_COMMAND_NAME_BYTES).collect();
            if let Some(idx) = out.find(char::is_whitespace) {
                out.truncate(idx);
            }
            out
        });

        let observation = AgentObservation {
            id: ObservationId(id),
            session,
            source: draft.source,
            evidence_kind: draft.evidence_kind,
            classification: draft.classification,
            strength: draft.strength,
            availability: draft.availability,
            agent,
            command: draft.command,
            command_name,
            observed_at_ms: draft.observed_at_ms,
            stale_after_ms: draft.stale_after_ms,
        };

        if self.observations.len() >= self.capacity {
            if let Some(old_obs) = self.observations.front() {
                if old_obs.source() == EvidenceSource::RecordedCommand {
                    if let Some(cmd) = old_obs.command() {
                        self.seen_recorded
                            .remove(&(old_obs.session().as_str().to_string(), cmd.0));
                    }
                }
            }
            self.observations.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.observations.push_back(observation.clone());
        Ok(observation)
    }

    pub(crate) fn try_record_trusted(
        &mut self,
        draft: ObservationDraft,
    ) -> Result<AgentObservation, ObservationError> {
        // Only the trusted recorder adapter may create RecordedCommand.
        match draft.source {
            EvidenceSource::RecordedCommand => {
                if draft.command.is_none()
                    || draft.evidence_kind != EvidenceKind::CommandLaunchCandidate
                    || draft.classification != ObservationClass::HeuristicCandidate
                    || draft.strength != EvidenceStrength::Candidate
                    || draft.agent.is_unknown()
                {
                    return Err(ObservationError::InvalidEvidence);
                }
            }
            _ => return Err(ObservationError::UnsupportedSource),
        }
        if draft.availability != EvidenceAvailability::Available {
            return Err(ObservationError::InvalidEvidence);
        }
        if draft.session.as_str().len() > MAX_SESSION_REF_BYTES {
            return Err(ObservationError::OversizedInput);
        }
        if let Some(name) = &draft.command_name {
            if name.len() > MAX_COMMAND_NAME_BYTES {
                return Err(ObservationError::OversizedInput);
            }
        }
        if let AgentKind::Unknown(name) = &draft.agent {
            if name.len() > MAX_AGENT_NAME_BYTES {
                return Err(ObservationError::OversizedInput);
            }
        }
        if draft.stale_after_ms < MIN_STALE_AFTER_MS || draft.stale_after_ms > MAX_STALE_AFTER_MS {
            return Err(ObservationError::InvalidTimestamp);
        }

        let id = if self.next_id == u64::MAX {
            return Err(ObservationError::IdExhausted);
        } else {
            let id = self.next_id;
            self.next_id += 1;
            id
        };

        let session = draft.session;
        let agent = draft.agent;
        let command_name = draft.command_name.map(|name| {
            let mut out: String = name.chars().take(MAX_COMMAND_NAME_BYTES).collect();
            if let Some(idx) = out.find(char::is_whitespace) {
                out.truncate(idx);
            }
            out
        });

        let observation = AgentObservation {
            id: ObservationId(id),
            session,
            source: draft.source,
            evidence_kind: draft.evidence_kind,
            classification: draft.classification,
            strength: draft.strength,
            availability: draft.availability,
            agent,
            command: draft.command,
            command_name,
            observed_at_ms: draft.observed_at_ms,
            stale_after_ms: draft.stale_after_ms,
        };

        // Evict dedup key with observation.
        if self.observations.len() >= self.capacity {
            if let Some(old_obs) = self.observations.front() {
                if old_obs.source() == EvidenceSource::RecordedCommand {
                    if let Some(cmd) = old_obs.command() {
                        self.seen_recorded
                            .remove(&(old_obs.session().as_str().to_string(), cmd.0));
                    }
                }
            }
            self.observations.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.observations.push_back(observation.clone());
        Ok(observation)
    }

    pub fn observe_command(
        &mut self,
        session: SessionRef,
        command_line: &str,
        now_ms: u64,
    ) -> Option<AgentObservation> {
        let name = first_command_token(command_line)?;
        let agent = match known_agent(&name) {
            Some(a) => a,
            None => {
                eprintln!("unknown agent {name}");
                return None;
            }
        };
        let session = SessionRef::new(session.as_str().to_string()).ok()?;
        let draft = ObservationDraft {
            session,
            source: EvidenceSource::CommandText,
            evidence_kind: EvidenceKind::CommandLaunchCandidate,
            classification: ObservationClass::HeuristicCandidate,
            strength: EvidenceStrength::Candidate,
            agent,
            command: None,
            command_name: Some(name),
            observed_at_ms: now_ms,
            stale_after_ms: 300_000,
            availability: EvidenceAvailability::Available,
        };
        self.try_record(draft).ok()
    }

    pub fn observe_recorded_command(
        &mut self,
        store: &zet_blocks::Store,
        session: SessionRef,
        block_id: i64,
        now_ms: u64,
    ) -> Option<AgentObservation> {
        if block_id <= 0 {
            return None;
        }
        let block = store.get(block_id).ok()?;
        if block.session != session.as_str() {
            return None;
        }
        let name = first_command_token(&block.command)?;
        let agent = known_agent(&name)?;
        let session = SessionRef::new(session.as_str().to_string()).ok()?;
        let seen_key = (session.as_str().to_string(), block_id);
        if !self.seen_recorded.insert(seen_key.clone()) {
            return None;
        }
        let draft = ObservationDraft {
            session,
            source: EvidenceSource::RecordedCommand,
            evidence_kind: EvidenceKind::CommandLaunchCandidate,
            classification: ObservationClass::HeuristicCandidate,
            strength: EvidenceStrength::Candidate,
            agent,
            command: Some(CommandRef(block_id)),
            command_name: Some(name),
            observed_at_ms: now_ms,
            stale_after_ms: 300_000,
            availability: EvidenceAvailability::Available,
        };
        match self.try_record_trusted(draft) {
            Ok(obs) => Some(obs),
            Err(_) => {
                self.seen_recorded.remove(&seen_key);
                None
            }
        }
    }

    pub fn record_recorder_dropped(&mut self, session: SessionRef, count: u64) {
        let key = session.as_str().to_string();
        if !self.recorded_dropped_by_session.contains_key(&key)
            && self.recorded_dropped_by_session.len() >= MAX_OBSERVATION_CAPACITY
        {
            self.recorded_loss_complete = false;
            return;
        }
        let previous = self
            .recorded_dropped_by_session
            .get(&key)
            .copied()
            .unwrap_or(0);
        let delta = if count >= previous {
            count - previous
        } else {
            count
        };
        self.recorded_dropped = self.recorded_dropped.saturating_add(delta);
        self.recorded_dropped_by_session.insert(key, count);
    }

    pub fn recorded_dropped(&self) -> u64 {
        self.recorded_dropped
    }

    pub fn recorded_dropped_by_session(&self) -> &HashMap<String, u64> {
        &self.recorded_dropped_by_session
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

    pub fn snapshot(&self, now_ms: u64) -> AgentObservationSnapshot {
        let observations: Vec<AgentObservation> = self
            .observations
            .iter()
            .map(|obs| {
                let mut obs = obs.clone();
                obs.availability = obs.effective_availability(now_ms);
                obs
            })
            .collect();
        AgentObservationSnapshot {
            observations,
            capacity: self.capacity,
            len: self.observations.len(),
            dropped: self.dropped,
            recorded_dropped: self.recorded_dropped,
            recorded_loss_complete: self.recorded_loss_complete,
            conflict_status: ConflictStatus::Unknown,
        }
    }

    pub fn clear(&mut self) {
        self.observations.clear();
        self.dropped = 0;
        self.recorded_dropped = 0;
        self.recorded_dropped_by_session.clear();
        self.recorded_loss_complete = true;
        self.seen_recorded.clear();
    }

    pub fn clear_session(&mut self, session: &SessionRef) {
        self.observations.retain(|o| o.session != *session);
        self.seen_recorded.retain(|(s, _)| s != session.as_str());
        self.recorded_dropped_by_session.remove(session.as_str());
    }
}

fn validate_draft(draft: &ObservationDraft) -> Result<(), ObservationError> {
    if draft.stale_after_ms < MIN_STALE_AFTER_MS || draft.stale_after_ms > MAX_STALE_AFTER_MS {
        return Err(ObservationError::InvalidTimestamp);
    }
    match draft.source {
        EvidenceSource::RecordedCommand => {
            // Public recording cannot manufacture persisted provenance.
            return Err(ObservationError::UnsupportedSource);
        }
        EvidenceSource::CommandText => {
            if draft.command.is_some() {
                return Err(ObservationError::InvalidEvidence);
            }
            if draft.evidence_kind != EvidenceKind::CommandLaunchCandidate
                || draft.classification != ObservationClass::HeuristicCandidate
                || draft.strength != EvidenceStrength::Candidate
            {
                return Err(ObservationError::InvalidEvidence);
            }
            if draft.agent.is_unknown() {
                return Err(ObservationError::InvalidEvidence);
            }
        }
        EvidenceSource::ProcessTable | EvidenceSource::StructuredIntegration => {
            return Err(ObservationError::UnsupportedSource);
        }
    }
    if draft.availability != EvidenceAvailability::Available {
        return Err(ObservationError::InvalidEvidence);
    }
    if draft.session.as_str().len() > MAX_SESSION_REF_BYTES {
        return Err(ObservationError::OversizedInput);
    }
    if let Some(name) = &draft.command_name {
        if name.len() > MAX_COMMAND_NAME_BYTES {
            return Err(ObservationError::OversizedInput);
        }
    }
    if let AgentKind::Unknown(name) = &draft.agent {
        if name.len() > MAX_AGENT_NAME_BYTES {
            return Err(ObservationError::OversizedInput);
        }
    }
    Ok(())
}

fn first_command_token(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
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
        // Bound before allocation.
        let bounded: String = token.chars().take(MAX_COMMAND_NAME_BYTES).collect();
        return Some(bounded);
    }
}

fn known_agent(name: &str) -> Option<AgentKind> {
    let base = name.rsplit('/').next().unwrap_or(name).to_lowercase();
    let base = base.trim_start_matches("./");
    if base == "opencode" {
        Some(AgentKind::OpenCode)
    } else if base == "gemini" {
        Some(AgentKind::GeminiCli)
    } else if base == "claude" {
        Some(AgentKind::ClaudeCode)
    } else if base == "codex" {
        Some(AgentKind::CodexCli)
    } else if base == "aider" {
        Some(AgentKind::Aider)
    } else if base == "cursor" {
        Some(AgentKind::Cursor)
    } else if base == "continue" || base == "cn" {
        Some(AgentKind::Continue)
    } else if base == "cline" {
        Some(AgentKind::Cline)
    } else if base == "copilot" {
        Some(AgentKind::Copilot)
    } else if base == "goose" {
        Some(AgentKind::Goose)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_command_does_not_become_observation() {
        let mut store = AgentObservationStore::new(8);
        assert!(store
            .observe_command(SessionRef::new("s1").unwrap(), "cargo test", 1000)
            .is_none());
        assert!(store.is_empty());
    }

    #[test]
    fn known_agent_command_is_candidate_only() {
        let mut store = AgentObservationStore::new(8);
        let obs = store
            .observe_command(SessionRef::new("s1").unwrap(), "opencode run foo", 1000)
            .expect("candidate");
        assert_eq!(obs.classification(), ObservationClass::HeuristicCandidate);
        assert_eq!(obs.strength(), EvidenceStrength::Candidate);
        assert_eq!(obs.evidence_kind(), EvidenceKind::CommandLaunchCandidate);
        assert_eq!(obs.agent(), &AgentKind::OpenCode);
        assert_eq!(obs.source(), EvidenceSource::CommandText);
        assert_eq!(obs.command(), None);
        assert_eq!(obs.availability(), EvidenceAvailability::Available);
    }

    #[test]
    fn unlinked_command_text_is_not_recorded_provenance() {
        let mut store = AgentObservationStore::new(8);
        let obs = store
            .observe_command(SessionRef::new("s1").unwrap(), "claude", 1000)
            .expect("candidate");
        assert_eq!(obs.source(), EvidenceSource::CommandText);
        assert_eq!(obs.command(), None);
    }

    #[test]
    fn unknown_agent_cli_is_not_observed() {
        let mut store = AgentObservationStore::new(8);
        assert!(store
            .observe_command(SessionRef::new("s1").unwrap(), "my-agent-cli do work", 1000)
            .is_none());
    }

    #[test]
    fn multiple_sessions_stay_separate() {
        let mut store = AgentObservationStore::new(8);
        store.observe_command(SessionRef::new("s1").unwrap(), "claude", 1000);
        store.observe_command(SessionRef::new("s2").unwrap(), "codex", 1000);
        assert_eq!(store.for_session(&SessionRef::new("s1").unwrap()).len(), 1);
        assert_eq!(store.for_session(&SessionRef::new("s2").unwrap()).len(), 1);
        assert_eq!(store.for_session(&SessionRef::new("s3").unwrap()).len(), 0);
    }

    #[test]
    fn bounded_history_drops_oldest() {
        let mut store = AgentObservationStore::new(2);
        store.observe_command(SessionRef::new("s").unwrap(), "opencode", 1);
        store.observe_command(SessionRef::new("s").unwrap(), "claude", 2);
        store.observe_command(SessionRef::new("s").unwrap(), "codex", 3);
        assert_eq!(store.len(), 2);
        assert_eq!(store.dropped(), 1);
    }

    #[test]
    fn sequential_agent_launches_are_not_conflicts() {
        let mut store = AgentObservationStore::new(8);
        store.observe_command(SessionRef::new("s").unwrap(), "opencode", 1000);
        store.observe_command(SessionRef::new("s").unwrap(), "gemini", 1001);
        let snap = store.snapshot(1001);
        assert_eq!(snap.conflict_status, ConflictStatus::Unknown);
        assert_eq!(snap.observations.len(), 2);
    }

    #[test]
    fn stale_candidates_do_not_imply_stopped_agent() {
        let mut store = AgentObservationStore::new(8);
        store.observe_command(SessionRef::new("s").unwrap(), "opencode", 1000);
        let snap = store.snapshot(1000 + 400_000);
        assert_eq!(
            snap.observations[0].availability(),
            EvidenceAvailability::Stale
        );
        assert_eq!(snap.conflict_status, ConflictStatus::Unknown);
    }

    #[test]
    fn snapshot_materializes_effective_availability() {
        let mut store = AgentObservationStore::new(8);
        store.observe_command(SessionRef::new("s").unwrap(), "opencode", 1000);
        let snap = store.snapshot(1000 + 400_000);
        assert_eq!(
            snap.observations[0].availability(),
            EvidenceAvailability::Stale
        );
    }

    #[test]
    fn future_dated_observation_is_not_stale() {
        let mut store = AgentObservationStore::new(8);
        store.observe_command(SessionRef::new("s").unwrap(), "opencode", 5000);
        let snap = store.snapshot(1000);
        assert_eq!(
            snap.observations[0].availability(),
            EvidenceAvailability::Available
        );
    }

    #[test]
    fn clock_boundary_is_safe() {
        let mut store = AgentObservationStore::new(8);
        store.observe_command(SessionRef::new("s").unwrap(), "opencode", u64::MAX - 10);
        let snap = store.snapshot(u64::MAX);
        assert_eq!(
            snap.observations[0].availability(),
            EvidenceAvailability::Stale
        );
    }

    #[test]
    fn capacity_is_clamped() {
        let store = AgentObservationStore::new(0);
        assert_eq!(store.capacity(), 1);
        let store = AgentObservationStore::new(usize::MAX);
        assert_eq!(store.capacity(), MAX_OBSERVATION_CAPACITY);
    }

    #[test]
    fn clear_session_removes_only_that_session() {
        let mut store = AgentObservationStore::new(8);
        store.observe_command(SessionRef::new("s1").unwrap(), "opencode", 1);
        store.observe_command(SessionRef::new("s2").unwrap(), "claude", 2);
        store.clear_session(&SessionRef::new("s1").unwrap());
        assert!(store
            .for_session(&SessionRef::new("s1").unwrap())
            .is_empty());
        assert_eq!(store.for_session(&SessionRef::new("s2").unwrap()).len(), 1);
    }

    #[test]
    fn command_name_is_bounded_and_argument_free() {
        let mut store = AgentObservationStore::new(8);
        let obs = store
            .observe_command(
                SessionRef::new("s").unwrap(),
                "opencode --secret-token=abc run",
                1,
            )
            .unwrap();
        assert_eq!(obs.command_name(), Some("opencode"));
    }

    #[test]
    fn invalid_evidence_combinations_are_rejected() {
        let mut store = AgentObservationStore::new(8);
        // Process source unsupported.
        let draft = ObservationDraft {
            session: SessionRef::new("s").unwrap(),
            source: EvidenceSource::ProcessTable,
            evidence_kind: EvidenceKind::ProcessPresent,
            classification: ObservationClass::ObservedFact,
            strength: EvidenceStrength::Verified,
            agent: AgentKind::OpenCode,
            command: None,
            command_name: None,
            observed_at_ms: 1,
            stale_after_ms: 300_000,
            availability: EvidenceAvailability::Available,
        };
        assert!(store.try_record(draft).is_err());

        // RecordedCommand without block id.
        let draft = ObservationDraft {
            session: SessionRef::new("s").unwrap(),
            source: EvidenceSource::RecordedCommand,
            evidence_kind: EvidenceKind::CommandLaunchCandidate,
            classification: ObservationClass::HeuristicCandidate,
            strength: EvidenceStrength::Candidate,
            agent: AgentKind::OpenCode,
            command: None,
            command_name: Some("opencode".into()),
            observed_at_ms: 1,
            stale_after_ms: 300_000,
            availability: EvidenceAvailability::Available,
        };
        assert!(store.try_record(draft).is_err());

        // Candidate upgraded to verified.
        let draft = ObservationDraft {
            session: SessionRef::new("s").unwrap(),
            source: EvidenceSource::CommandText,
            evidence_kind: EvidenceKind::CommandLaunchCandidate,
            classification: ObservationClass::HeuristicCandidate,
            strength: EvidenceStrength::Verified,
            agent: AgentKind::OpenCode,
            command: None,
            command_name: Some("opencode".into()),
            observed_at_ms: 1,
            stale_after_ms: 300_000,
            availability: EvidenceAvailability::Available,
        };
        assert!(store.try_record(draft).is_err());

        // Unknown agent cannot be recorded as candidate.
        let draft = ObservationDraft {
            session: SessionRef::new("s").unwrap(),
            source: EvidenceSource::CommandText,
            evidence_kind: EvidenceKind::CommandLaunchCandidate,
            classification: ObservationClass::HeuristicCandidate,
            strength: EvidenceStrength::Candidate,
            agent: AgentKind::Unknown("mystery".into()),
            command: None,
            command_name: Some("mystery".into()),
            observed_at_ms: 1,
            stale_after_ms: 300_000,
            availability: EvidenceAvailability::Available,
        };
        assert!(store.try_record(draft).is_err());
    }

    #[test]
    fn deserialized_invalid_record_is_rejected() {
        let json = r#"{
            "id": 1,
            "session": "s",
            "source": "ProcessTable",
            "evidence_kind": "ProcessPresent",
            "classification": "ObservedFact",
            "strength": "Verified",
            "availability": "Available",
            "agent": "OpenCode",
            "command": null,
            "command_name": null,
            "observed_at_ms": 1,
            "stale_after_ms": 300000
        }"#;
        let dto: Result<AgentObservationDto, _> = serde_json::from_str(json);
        assert!(dto.is_ok());
        let store = zet_blocks::Store::open_in_memory().unwrap();
        assert!(AgentObservation::from_dto(dto.unwrap(), Some(&store)).is_err());
    }

    #[test]
    fn adversarial_command_syntax_does_not_upgrade() {
        let mut store = AgentObservationStore::new(8);
        // Wrapper should not be recognized as an agent.
        assert!(store
            .observe_command(SessionRef::new("s").unwrap(), "sudo opencode", 1)
            .is_none());
        // Unknown near-match is not recognized.
        assert!(store
            .observe_command(SessionRef::new("s").unwrap(), "opencodee", 1)
            .is_none());
    }

    #[test]
    fn oversized_inputs_are_bounded() {
        let mut store = AgentObservationStore::new(8);
        let long_session = "x".repeat(1000);
        assert!(SessionRef::new(long_session).is_err());

        let draft = ObservationDraft {
            session: SessionRef::new("s").unwrap(),
            source: EvidenceSource::CommandText,
            evidence_kind: EvidenceKind::CommandLaunchCandidate,
            classification: ObservationClass::HeuristicCandidate,
            strength: EvidenceStrength::Candidate,
            agent: AgentKind::Unknown("y".repeat(1000)),
            command: None,
            command_name: Some("z".repeat(1000)),
            observed_at_ms: 1,
            stale_after_ms: 300_000,
            availability: EvidenceAvailability::Available,
        };
        assert!(store.try_record(draft).is_err());
    }

    #[test]
    fn recorded_command_requires_persisted_block() {
        let store = zet_blocks::Store::open_in_memory().unwrap();
        let id = store
            .append(&zet_blocks::Block {
                id: 0,
                session: "s".to_string(),
                command: "opencode run".to_string(),
                output: String::new(),
                exit_code: 0,
                cwd: "/tmp".to_string(),
                git_branch: String::new(),
                duration_ms: 1,
            })
            .unwrap();
        let mut obs_store = AgentObservationStore::new(8);
        let session = SessionRef::new("s").unwrap();
        let obs = obs_store.observe_recorded_command(&store, session.clone(), id, 10);
        assert!(obs.is_some());
        assert_eq!(obs.unwrap().command(), Some(CommandRef(id)));
    }

    #[test]
    fn wrong_session_block_is_rejected() {
        let store = zet_blocks::Store::open_in_memory().unwrap();
        let id = store
            .append(&zet_blocks::Block {
                id: 0,
                session: "other".to_string(),
                command: "opencode run".to_string(),
                output: String::new(),
                exit_code: 0,
                cwd: "/tmp".to_string(),
                git_branch: String::new(),
                duration_ms: 1,
            })
            .unwrap();
        let mut obs_store = AgentObservationStore::new(8);
        let session = SessionRef::new("s").unwrap();
        assert!(obs_store
            .observe_recorded_command(&store, session, id, 10)
            .is_none());
    }

    #[test]
    fn missing_block_is_rejected() {
        let store = zet_blocks::Store::open_in_memory().unwrap();
        let mut obs_store = AgentObservationStore::new(8);
        let session = SessionRef::new("s").unwrap();
        assert!(obs_store
            .observe_recorded_command(&store, session, 999, 10)
            .is_none());
    }

    #[test]
    fn unicode_session_ref_does_not_panic() {
        let cases = [
            "a".repeat(126) + "😀",
            "b".repeat(125) + "中",
            "c".repeat(127) + "é",
        ];
        for case in cases {
            let result = SessionRef::new(case);
            assert!(result.is_err() || result.unwrap().as_str().len() <= MAX_SESSION_REF_BYTES);
        }
        assert!(SessionRef::new("session-😀-中".to_string()).is_ok());
    }

    #[test]
    fn session_ref_rejects_overlong() {
        assert!(SessionRef::new("x".repeat(MAX_SESSION_REF_BYTES + 1)).is_err());
    }

    #[test]
    fn bounded_dedup_evicts_with_observations() {
        let store = zet_blocks::Store::open_in_memory().unwrap();
        let id = store
            .append(&zet_blocks::Block {
                id: 0,
                session: "s".to_string(),
                command: "opencode run".to_string(),
                output: String::new(),
                exit_code: 0,
                cwd: "/tmp".to_string(),
                git_branch: String::new(),
                duration_ms: 1,
            })
            .unwrap();
        let mut obs_store = AgentObservationStore::new(1);
        let session = SessionRef::new("s").unwrap();
        assert!(obs_store
            .observe_recorded_command(&store, session.clone(), id, 1)
            .is_some());
        // Fill capacity with an ordinary command observation, evicting the recorded one.
        obs_store.observe_command(session.clone(), "claude", 2);
        // Duplicate delivery after eviction is allowed again (no permanent exactly-once).
        assert!(obs_store
            .observe_recorded_command(&store, session, id, 3)
            .is_some());
    }

    #[test]
    fn clear_releases_dedup_state() {
        let store = zet_blocks::Store::open_in_memory().unwrap();
        let id = store
            .append(&zet_blocks::Block {
                id: 0,
                session: "s".to_string(),
                command: "opencode run".to_string(),
                output: String::new(),
                exit_code: 0,
                cwd: "/tmp".to_string(),
                git_branch: String::new(),
                duration_ms: 1,
            })
            .unwrap();
        let mut obs_store = AgentObservationStore::new(8);
        let session = SessionRef::new("s").unwrap();
        assert!(obs_store
            .observe_recorded_command(&store, session.clone(), id, 1)
            .is_some());
        obs_store.clear_session(&session);
        assert!(obs_store
            .observe_recorded_command(&store, session, id, 2)
            .is_some());
    }

    #[test]
    fn multi_session_loss_is_aggregated() {
        let mut store = AgentObservationStore::new(8);
        let s1 = SessionRef::new("s1").unwrap();
        let s2 = SessionRef::new("s2").unwrap();
        store.record_recorder_dropped(s1.clone(), 2);
        store.record_recorder_dropped(s2.clone(), 3);
        assert_eq!(store.recorded_dropped(), 5);
        // Repeated snapshot does not double count.
        store.record_recorder_dropped(s1.clone(), 2);
        assert_eq!(store.recorded_dropped(), 5);
        // Increased count adds only the delta.
        store.record_recorder_dropped(s1, 4);
        assert_eq!(store.recorded_dropped(), 7);
        // Counter reset adds the new counter value as fresh loss.
        store.record_recorder_dropped(s2.clone(), 1);
        assert_eq!(store.recorded_dropped(), 8);
        // Session retirement releases active bookkeeping but preserves
        // historical loss.
        store.clear_session(&s2);
        assert_eq!(store.recorded_dropped(), 8);
        assert!(!store.recorded_dropped_by_session().contains_key("s2"));
    }

    #[test]
    fn loss_accounting_is_bounded() {
        let mut store = AgentObservationStore::new(2);
        for i in 0..5 {
            store.record_recorder_dropped(SessionRef::new(format!("s{i}")).unwrap(), 1);
        }
        assert!(store.recorded_dropped_by_session().len() <= MAX_OBSERVATION_CAPACITY);
    }
    #[test]
    fn id_exhaustion_is_safe() {
        // Cannot allocate u64::MAX ids in a test. Instead, verify the
        // checked increment path rejects an exhausted counter.
        let mut store = AgentObservationStore::new(1);
        store.next_id = u64::MAX;
        let draft = ObservationDraft {
            session: SessionRef::new("s").unwrap(),
            source: EvidenceSource::CommandText,
            evidence_kind: EvidenceKind::CommandLaunchCandidate,
            classification: ObservationClass::HeuristicCandidate,
            strength: EvidenceStrength::Candidate,
            agent: AgentKind::OpenCode,
            command: None,
            command_name: Some("opencode".into()),
            observed_at_ms: 1,
            stale_after_ms: 300_000,
            availability: EvidenceAvailability::Available,
        };
        assert!(matches!(
            store.try_record(draft),
            Err(ObservationError::IdExhausted)
        ));
    }
}
