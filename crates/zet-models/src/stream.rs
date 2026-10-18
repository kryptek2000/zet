// Streaming half of the model gateway: one `chat_stream` trait, every
// provider behind it (docs/ZET-DESIGN.md §8). Deltas leave through a
// callback, so the transport stays blocking — the UI paints text as it
// lands, no async runtime anywhere in the workspace.
//
// Two kinds of implementation:
//   - OpenAI-compatible SSE (`openai.rs`) reassembles `data:` frames and
//     emits per-chunk deltas.
//   - Everything else inherits the default below: blocking `chat`, then
//     the finished answer replayed as events. Same answer, no partial
//     paint. Every existing `ChatModel` (test mocks included) therefore
//     streams with zero changes.

use serde_json::Value;

use crate::{ChatModel, ChatRequest, ChatResponse, ModelError, ToolCall};

/// What a streaming turn hands the UI as it happens. `ToolCallDelta`
/// arrives per index and per chunk: `id` and `name` ride the first
/// fragment, `arguments` arrive in pieces to be concatenated.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    TextDelta(String),
    ToolCallDelta {
        index: u32,
        id: Option<String>,
        name: Option<String>,
        arguments: String,
    },
    Done,
}

pub trait ChatStream: ChatModel {
    /// Stream one turn. `on_event` sees every delta in order, then
    /// exactly one `Done`; the returned response is the whole answer
    /// assembled, identical in content to what `chat` would have
    /// returned for the same request.
    fn chat_stream(
        &self,
        req: &ChatRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<ChatResponse, ModelError> {
        let res = self.chat(req)?;
        if !res.text.is_empty() {
            on_event(StreamEvent::TextDelta(res.text.clone()));
        }
        for (index, tc) in res.tool_calls.iter().enumerate() {
            on_event(StreamEvent::ToolCallDelta {
                index: index as u32,
                id: Some(tc.id.clone()),
                name: Some(tc.name.clone()),
                arguments: tc.arguments.clone(),
            });
        }
        on_event(StreamEvent::Done);
        Ok(res)
    }
}

/// A boxed streaming client is also a plain blocking one, so it can sit
/// anywhere a `ChatModel` is expected (the agent is generic over one).
impl ChatModel for Box<dyn ChatStream> {
    fn chat(&self, req: &ChatRequest) -> Result<ChatResponse, ModelError> {
        (**self).chat(req)
    }
}

impl ChatStream for Box<dyn ChatStream> {
    fn chat_stream(
        &self,
        req: &ChatRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<ChatResponse, ModelError> {
        (**self).chat_stream(req, on_event)
    }
}

/// One tool call while its arguments stream in. `arguments` is a
/// fragment until the stream ends: `{"pa` + `th":"a.txt"}` is one call.
#[derive(Debug, Default, PartialEq)]
struct PartialCall {
    id: String,
    name: String,
    arguments: String,
}

/// Upper bound on a streamed tool-call index. No model asks for more
/// than a handful of parallel calls; the number only exists to key the
/// accumulator, so an absurd one is a broken (or hostile) server.
const MAX_CALL_INDEX: usize = 1024;

/// What one SSE line did.
#[derive(Debug, Clone, PartialEq)]
pub enum SseStep {
    /// Events to hand the UI. Empty for framing lines (comments,
    /// `event:`, blank separators) — those are not payload.
    Events(Vec<StreamEvent>),
    /// The terminal `[DONE]` frame. Nothing after it is part of the answer.
    Done,
}

/// SSE reassembly: `data:` frames in, `ChatResponse` out, deltas as they
/// land. No I/O and no callback, so the fiddly parts (arguments split
/// mid-JSON, null content, interleaved tool indices, a stream cut in
/// half) are testable without a socket.
#[derive(Debug, Default)]
pub struct SseAccumulator {
    text: String,
    /// Sparse by delta index: a server may number calls out of order, and
    /// a missing index is a hole, not a shift.
    calls: Vec<Option<PartialCall>>,
    done: bool,
}

impl SseAccumulator {
    pub fn new() -> Self {
        SseAccumulator::default()
    }

    /// True once `[DONE]` closed the stream; later lines are ignored.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Feed one raw line (trailing newline optional). Lines that carry no
    /// `data:` field are framing, not payload, and pass through as an
    /// empty event list; a `data:` field that is neither `[DONE]` nor
    /// JSON is a protocol violation, not something to guess at.
    pub fn push_line(&mut self, line: &str) -> Result<SseStep, ModelError> {
        if self.done {
            return Ok(SseStep::Done);
        }
        let Some(payload) = line.trim_end_matches(['\r', '\n']).strip_prefix("data:") else {
            return Ok(SseStep::Events(Vec::new()));
        };
        let payload = payload.trim();
        if payload == "[DONE]" {
            self.done = true;
            return Ok(SseStep::Done);
        }
        let chunk: Value = serde_json::from_str(payload)
            .map_err(|e| ModelError::Protocol(format!("bad SSE chunk: {e}")))?;
        if chunk.get("error").is_some() {
            // Mid-stream errors arrive as data, not as a status code.
            let body = chunk.to_string();
            let msg = crate::openai::error_message(&body)
                .unwrap_or_else(|| crate::openai::flatten(&body));
            return Err(ModelError::Protocol(format!("stream error: {msg}")));
        }

        let choice = chunk.pointer("/choices/0");
        // `delta` is the streaming shape; some compat servers ignore
        // "stream": true and answer with one whole `message` — same
        // fields, so read it as the delta it stands in for.
        let frame = choice
            .and_then(|c| c.get("delta"))
            .or_else(|| choice.and_then(|c| c.get("message")));

        let mut events = Vec::new();
        if let Some(content) = frame
            .and_then(|d| d.get("content"))
            .and_then(|c| c.as_str())
            .filter(|c| !c.is_empty())
        {
            self.text.push_str(content);
            events.push(StreamEvent::TextDelta(content.to_string()));
        }
        if let Some(calls) = frame
            .and_then(|d| d.get("tool_calls"))
            .and_then(|c| c.as_array())
        {
            for call in calls {
                let index = call
                    .get("index")
                    .and_then(|i| i.as_u64())
                    .map(|i| i as usize)
                    .unwrap_or(self.calls.len());
                // The index sizes a vec: a server claiming index 2^40
                // would ask for its memory, not for a tool call.
                if index > MAX_CALL_INDEX {
                    return Err(ModelError::Protocol(format!(
                        "tool call index {index} is out of range"
                    )));
                }
                if self.calls.len() <= index {
                    self.calls.resize_with(index + 1, || None);
                }
                let function = call.get("function").cloned().unwrap_or(Value::Null);
                let id = call.get("id").and_then(|v| v.as_str()).map(str::to_string);
                let name = function
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let arguments = match function.get("arguments") {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Null) | None => String::new(),
                    Some(other) => other.to_string(),
                };
                let slot = self.calls[index].get_or_insert_with(PartialCall::default);
                if let Some(id) = &id {
                    slot.id.clone_from(id);
                }
                if let Some(name) = &name {
                    slot.name.clone_from(name);
                }
                slot.arguments.push_str(&arguments);
                // Index-only frames (some servers open every call with
                // one) carry nothing the model hasn't already been told.
                if id.is_none() && name.is_none() && arguments.is_empty() {
                    continue;
                }
                events.push(StreamEvent::ToolCallDelta {
                    index: index as u32,
                    id,
                    name,
                    arguments,
                });
            }
        }
        Ok(SseStep::Events(events))
    }

    /// The assembled answer. Errors when the stream left a call
    /// unfinished: a nameless call, or arguments that never became JSON
    /// once the fragments were concatenated (the agent would fail on
    /// the same string later, with no way to tell it was the wire).
    pub fn finish(self) -> Result<ChatResponse, ModelError> {
        let mut tool_calls = Vec::new();
        for slot in self.calls.into_iter().flatten() {
            if slot.name.is_empty() {
                return Err(ModelError::Protocol(
                    "streamed tool call never got a name".to_string(),
                ));
            }
            if !slot.arguments.trim().is_empty()
                && serde_json::from_str::<Value>(&slot.arguments).is_err()
            {
                return Err(ModelError::Protocol(format!(
                    "streamed arguments for '{}' did not reassemble into JSON",
                    slot.name
                )));
            }
            tool_calls.push(ToolCall {
                id: slot.id,
                name: slot.name,
                arguments: slot.arguments,
            });
        }
        Ok(ChatResponse {
            text: self.text,
            tool_calls,
        })
    }
}
