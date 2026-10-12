// Command suggestions: instant history completion first, local model for
// NL-to-command and prefix completion when history misses. History never
// touches the network, so ghost text stays instant; the model only runs
// on explicit NL requests or history misses the UI opts into.

use zet_blocks::Store;
use zet_models::{ChatModel, ChatRequest, Message, Role};

pub mod ghost;
pub use ghost::{Ghost, PromptInput};

pub trait Suggest {
    /// Complete `prefix`, or None when nothing fits.
    fn suggest(&self, prefix: &str) -> Option<String>;
}

pub struct HistorySuggester<'a> {
    store: &'a Store,
}

impl<'a> HistorySuggester<'a> {
    pub fn new(store: &'a Store) -> Self {
        HistorySuggester { store }
    }
}

impl Suggest for HistorySuggester<'_> {
    fn suggest(&self, prefix: &str) -> Option<String> {
        self.store.complete(prefix).unwrap_or(None)
    }
}

pub struct ModelSuggester<M: ChatModel> {
    model: M,
}

impl<M: ChatModel> ModelSuggester<M> {
    pub fn new(model: M) -> Self {
        ModelSuggester { model }
    }

    fn ask(
        &self,
        system: &str,
        input: &str,
        context: Option<&str>,
        max_tokens: u32,
    ) -> Result<Option<String>, zet_models::ModelError> {
        let mut full_system = system.to_string();
        if let Some(context) = context {
            full_system.push_str("\nTerminal context (what the user sees):\n");
            full_system.push_str(context);
        }
        let req = ChatRequest {
            model: String::new(),
            system: Some(full_system),
            messages: vec![Message {
                role: Role::User,
                text: input.to_string(),
                tool_call_id: None,
                tool_calls: vec![],
            }],
            tools: vec![],
            max_tokens,
        };
        let res = self.model.chat(&req)?;
        Ok(first_line(&res.text))
    }

    /// Natural language to shell command. Replies with only the command —
    /// or CHAT: + one liner when there is no command (greeting/smalltalk).
    /// Model failures surface (the window notices them) instead of
    /// dissolving into silence. `context` (terminal state) rides along
    /// when present; the ghost path never pays for it.
    pub fn suggest_nl(
        &self,
        nl: &str,
        context: Option<&str>,
    ) -> Result<Option<String>, zet_models::ModelError> {
        self.ask(
            "Translate the request to a single shell command. Reply with ONLY the command, no prose, no fences. Examples: 'list files' -> ls -la; 'disk usage' -> df -h; 'find todos' -> grep -rn TODO . If the request is greeting, smalltalk, or has no shell command (e.g. 'hello'), reply with CHAT: followed by a friendly one-liner, e.g. 'CHAT: Hey! Tell me what to run.'",
            nl,
            context,
            // Room to read context + think + answer in one line. The old
            // 64-token budget starved reasoning models (thinking ate the
            // answer); a cap is not spend — one-liners still cost lines.
            256,
        )
    }
}

impl<M: ChatModel> Suggest for ModelSuggester<M> {
    fn suggest(&self, prefix: &str) -> Option<String> {
        if prefix.is_empty() {
            return None;
        }
        // Ghost path stays silent on failure (instant history covers it).
        // No context, tight budget: per-keystroke calls stay cheap.
        self.ask(
            "Complete this shell command prefix. Reply with ONLY the full command, no prose, no fences.",
            prefix,
            None,
            64,
        )
        .ok()
        .flatten()
        .filter(|c| c != prefix)
    }
}

/// History first (free, instant); model only on history miss.
pub struct ChainSuggester<H: Suggest, M: ChatModel> {
    history: H,
    model: ModelSuggester<M>,
}

impl<H: Suggest, M: ChatModel> ChainSuggester<H, M> {
    pub fn new(history: H, model: ModelSuggester<M>) -> Self {
        ChainSuggester { history, model }
    }

    pub fn suggest_nl(
        &self,
        nl: &str,
        context: Option<&str>,
    ) -> Result<Option<String>, zet_models::ModelError> {
        if let Some(hit) = self.history.suggest(nl) {
            return Ok(Some(hit));
        }
        self.model.suggest_nl(nl, context)
    }
}

impl<H: Suggest, M: ChatModel> Suggest for ChainSuggester<H, M> {
    fn suggest(&self, prefix: &str) -> Option<String> {
        if let Some(hit) = self.history.suggest(prefix) {
            return Some(hit);
        }
        self.model.suggest(prefix)
    }
}

fn first_line(text: &str) -> Option<String> {
    // Reasoning models (qwen3, deepseek-r1) wrap chain-of-thought in
    // <think> blocks, sometimes unclosed when cut off: drop everything
    // from the opener before looking for the command.
    let text = match text.find("<think>") {
        Some(i) => match text[i..].find("</think>") {
            Some(j) => format!("{}{}", &text[..i], &text[i + j + "</think>".len()..]),
            None => text[..i].to_string(),
        },
        None => text.to_string(),
    };
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        // Skip prose lead-ins ("Sure thing!", "Here you go:"). Commands
        // ending in !?: are the accepted casualty — EXCEPT when the
        // whole reply is one line: a lone `Hello!` is an answer, not a
        // lead-in, and skipping it dissolves into `(no command)`.
        if line.ends_with([':', '!', '?'])
            && text.lines().filter(|l| !l.trim().is_empty()).count() > 1
        {
            continue;
        }
        // Fence openers (``` or ```sh) carry no command; closed inline
        // fences (```cmd```) and unclosed multi-word fences do.
        if let Some(rest) = line.strip_prefix("```") {
            let rest = rest.trim();
            if rest.is_empty() {
                continue;
            }
            if let Some(cmd) = rest.strip_suffix("```") {
                let cmd = cmd.trim();
                if cmd.is_empty() {
                    continue;
                }
                return Some(cmd.to_string());
            }
            if rest.contains([' ', '\t']) {
                return Some(rest.to_string());
            }
            continue;
        }
        // One pair of inline-code backticks (`ls -a`), not command
        // substitution (echo `date` keeps its backticks).
        let clean = line.trim_start_matches(['$', '>', '-', '*']).trim();
        let clean = match clean.strip_prefix('`') {
            Some(inner) => inner.strip_suffix('`').unwrap_or(inner),
            None => clean,
        }
        .trim();
        if clean.is_empty() {
            continue;
        }
        return Some(clean.to_string());
    }
    None
}

/// A model reply that is conversation, not a command: "CHAT: <text>".
/// The window shows it as an `agent:` notice instead of swapping the
/// line — so `hello` gets answered, not exit 127. Passes through
/// first_line untouched (ends with `.`, no fences).
pub fn chat_reply(answer: &str) -> Option<String> {
    let rest = answer.strip_prefix("CHAT:")?;
    let text: String = rest.trim().chars().take(120).collect();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Visible ghost suffix: first line of `suggestion` after `prefix`.
///
/// History commands can span lines (continuation rows join with \n); the
/// ghost must be single-line, since Tab types it raw and an embedded LF
/// would execute the prefix early. None on exact match, non-prefix, or a
/// prefix that itself spans lines (typed input never contains \n).
pub fn ghost_suffix<'a>(prefix: &str, suggestion: &'a str) -> Option<&'a str> {
    if prefix.contains('\n') {
        return None;
    }
    let rest = suggestion.lines().next()?.strip_prefix(prefix)?;
    if rest.is_empty() {
        return None;
    }
    Some(rest)
}
