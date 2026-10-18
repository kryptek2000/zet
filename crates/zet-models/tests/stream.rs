// Streaming gateway: SSE reassembly and provider error mapping.
// Everything here is offline — the socket stubs bind 127.0.0.1:0 and
// answer with canned bytes; no key is ever sent anywhere real.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

use zet_models::openai::OpenAICompat;
use zet_models::{
    ChatRequest, ChatStream, Message, ModelError, Role, SseAccumulator, SseStep, StreamEvent,
    ToolCall,
};

/// A turn cut the way a provider cuts it: an opening frame, text in two
/// pieces, a frame with null content that opens a tool call, the tool
/// arguments split *mid-JSON*, a keep-alive comment, then `[DONE]`.
const CANNED: &[&str] = &[
    ": keep-alive",
    r#"data: {"choices":[{"index":0,"delta":{"role":"assistant","content":""},"finish_reason":null}]}"#,
    r#"data: {"choices":[{"index":0,"delta":{"content":"Hel"}}]}"#,
    r#"data: {"choices":[{"index":0,"delta":{"content":null,"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"pa"}}]}}]}"#,
    r#"data: {"choices":[{"index":0,"delta":{"content":"lo"}}]}"#,
    r#"data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"a.txt\"}"}}]}}]}"#,
    "",
    r#"event: done"#,
    "data: [DONE]",
];

/// Feed a canned stream through the accumulator, collecting events.
fn replay(lines: &[&str]) -> (Vec<StreamEvent>, Option<zet_models::ChatResponse>) {
    let mut acc = SseAccumulator::new();
    let mut events = Vec::new();
    for line in lines {
        match acc.push_line(line).unwrap() {
            SseStep::Events(evs) => events.extend(evs),
            SseStep::Done => break,
        }
    }
    (events, Some(acc.finish().unwrap()))
}

fn text_of(events: &[StreamEvent]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::TextDelta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn sse_reassembles_text_and_arguments_split_mid_json() {
    let (events, res) = replay(CANNED);
    let res = res.expect("finished");

    assert_eq!(res.text, "Hello");
    assert_eq!(
        res.tool_calls,
        vec![ToolCall {
            id: "call_1".to_string(),
            name: "read_file".to_string(),
            arguments: r#"{"path":"a.txt"}"#.to_string(),
        }]
    );
    // Deltas, not a final blob: the caller can paint as they arrive.
    assert_eq!(text_of(&events), "Hello");
    let fragments: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::ToolCallDelta { arguments, .. } => Some(arguments.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(fragments, vec![r#"{"pa"#, r#"th":"a.txt"}"#]);
}

#[test]
fn framing_lines_carry_no_events() {
    let mut acc = SseAccumulator::new();
    for line in [": ping", "event: message", "", "id: 7", "\r"] {
        assert_eq!(acc.push_line(line).unwrap(), SseStep::Events(Vec::new()));
    }
    assert!(!acc.is_done());
    assert_eq!(acc.finish().unwrap().text, "");
}

#[test]
fn done_ends_the_stream_and_ignores_the_rest() {
    let mut acc = SseAccumulator::new();
    acc.push_line(r#"data: {"choices":[{"delta":{"content":"hi"}}]}"#)
        .unwrap();
    assert_eq!(acc.push_line("data: [DONE]").unwrap(), SseStep::Done);
    assert!(acc.is_done());
    // A frame after [DONE] belongs to no answer.
    assert_eq!(
        acc.push_line(r#"data: {"choices":[{"delta":{"content":"late"}}]}"#)
            .unwrap(),
        SseStep::Done
    );
    assert_eq!(acc.finish().unwrap().text, "hi");
}

#[test]
fn garbage_payloads_are_protocol_errors() {
    let mut acc = SseAccumulator::new();
    let err = acc.push_line("data: {not json").unwrap_err();
    assert!(matches!(err, ModelError::Protocol(_)), "{err:?}");

    // Errors arrive mid-stream as data, not as a status code.
    let mut acc = SseAccumulator::new();
    let err = acc
        .push_line(r#"data: {"error":{"message":"model overloaded","type":"server_error"}}"#)
        .unwrap_err();
    assert!(matches!(err, ModelError::Protocol(_)), "{err:?}");
    assert!(err.to_string().contains("model overloaded"), "{err}");
}

#[test]
fn a_stream_that_cut_a_call_short_fails_loudly() {
    // Arguments that never became JSON: the agent would choke on the same
    // string later with no way to tell it came off a broken wire.
    let mut acc = SseAccumulator::new();
    acc.push_line(
        r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"grep","arguments":"{\"pattern\":"}}]}}]}"#,
    )
    .unwrap();
    acc.push_line("data: [DONE]").unwrap();
    let err = acc.finish().unwrap_err();
    assert!(matches!(err, ModelError::Protocol(_)), "{err:?}");

    // A call that never got a name is equally unusable.
    let mut acc = SseAccumulator::new();
    acc.push_line(
        r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"arguments":"{}"}}]}}]}"#,
    )
    .unwrap();
    acc.push_line("data: [DONE]").unwrap();
    let err = acc.finish().unwrap_err();
    assert!(matches!(err, ModelError::Protocol(_)), "{err:?}");
}

#[test]
fn an_absurd_call_index_is_refused() {
    // The index sizes a vec: a broken server must not get to allocate.
    let mut acc = SseAccumulator::new();
    let err = acc
        .push_line(
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":4000000000,"id":"c1","function":{"name":"grep","arguments":"{}"}}]}}]}"#,
        )
        .unwrap_err();
    assert!(matches!(err, ModelError::Protocol(_)), "{err:?}");
}

#[test]
fn several_calls_keep_their_own_indices() {
    let mut acc = SseAccumulator::new();
    // Second index first, then index 0: indices address calls, they are
    // not an order.
    acc.push_line(
        r#"data: {"choices":[{"delta":{"tool_calls":[{"index":1,"id":"c2","function":{"name":"glob","arguments":"{\"pattern\":\"*.rs\"}"}}]}}]}"#,
    )
    .unwrap();
    acc.push_line(
        r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"grep","arguments":"{}"}}]}}]}"#,
    )
    .unwrap();
    acc.push_line("data: [DONE]").unwrap();
    let res = acc.finish().unwrap();
    assert_eq!(
        res.tool_calls
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        vec!["grep", "glob"]
    );
}

/// A socket that answers one request with `status` + a raw body, and
/// hands back what it read. No Content-Length on a 200: the client has to
/// read the stream until the connection closes, like a real SSE reply.
fn serve(
    listener: TcpListener,
    status: u16,
    reason: &'static str,
    body: Vec<u8>,
) -> (mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if stream.read_exact(&mut byte).is_err() {
                break;
            }
            head.push(byte[0]);
        }
        let head_text = String::from_utf8_lossy(&head).to_lowercase();
        let len: usize = head_text
            .lines()
            .find_map(|l| {
                l.strip_prefix("content-length:")
                    .map(|v| v.trim().parse().unwrap_or(0))
            })
            .unwrap_or(0);
        let mut request = vec![0u8; len];
        stream.read_exact(&mut request).unwrap();
        let _ = tx
            .send(String::from_utf8_lossy(&head).into_owned() + &String::from_utf8_lossy(&request));
        write!(
            stream,
            "HTTP/1.1 {status} {reason}\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        // Frame at a time, flushed: a real stream, not one buffered blob.
        for frame in body.split_inclusive(|b| *b == b'\n') {
            stream.write_all(frame).unwrap();
            stream.flush().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    });
    (rx, handle)
}

fn bare_req() -> ChatRequest {
    ChatRequest {
        model: "stub-model".to_string(),
        system: None,
        messages: vec![Message {
            role: Role::User,
            text: "hi".to_string(),
            tool_call_id: None,
            tool_calls: vec![],
        }],
        tools: vec![],
        max_tokens: 8,
    }
}

#[test]
fn chat_stream_reads_a_real_sse_response() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let body = CANNED.join("\n").into_bytes();
    let (request, server) = serve(listener, 200, "OK", body);

    let client = OpenAICompat::new(
        format!("http://{addr}"),
        "sk-test".to_string(),
        "stub-model".to_string(),
    );
    let mut events = Vec::new();
    let res = client
        .chat_stream(&bare_req(), &mut |event| events.push(event))
        .unwrap();
    assert_eq!(res.text, "Hello");
    assert_eq!(res.tool_calls[0].arguments, r#"{"path":"a.txt"}"#);
    assert_eq!(text_of(&events), "Hello");
    assert_eq!(events.last(), Some(&StreamEvent::Done));
    assert!(events.len() >= 4, "expected deltas, got {:?}", events);
    server.join().unwrap();

    // The request asked for a stream, and said it accepts one.
    let request = request.recv().unwrap();
    let (head, body) = request.split_once("\r\n\r\n").expect("request had a body");
    assert!(
        head.to_lowercase().contains("accept: text/event-stream"),
        "{head}"
    );
    let payload: serde_json::Value = serde_json::from_str(body).expect("json request body");
    assert_eq!(payload["stream"], true, "{body}");
}

#[test]
fn chat_stream_maps_provider_statuses() {
    for (status, reason) in [
        (401u16, "Unauthorized"),
        (429, "Too Many Requests"),
        (500, "Bad Gateway"),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let body = br#"{"error":{"message":"nope"}}"#.to_vec();
        let (_request, server) = serve(listener, status, reason, body);

        let client = OpenAICompat::new(
            format!("http://{addr}"),
            "sk-test".to_string(),
            "stub-model".to_string(),
        );
        let mut seen = Vec::new();
        let err = client
            .chat_stream(&bare_req(), &mut |event| seen.push(event))
            .unwrap_err();
        let want = match status {
            401 => ModelError::Auth(String::new()),
            429 => ModelError::RateLimited(String::new()),
            _ => ModelError::Server(String::new()),
        };
        assert_eq!(
            std::mem::discriminant(&err),
            std::mem::discriminant(&want),
            "status {status} gave {err:?}"
        );
        // A failed request produces no answer, so no Done either.
        assert!(seen.is_empty(), "{seen:?}");
        server.join().unwrap();
    }
}

#[test]
fn chat_stream_reports_a_broken_stream_as_protocol() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let body = b"data: {\"choices\":[{\"delta\":\ndata: [DONE]\n".to_vec();
    let (_request, server) = serve(listener, 200, "OK", body);

    let client = OpenAICompat::new(
        format!("http://{addr}"),
        "sk-test".to_string(),
        "stub-model".to_string(),
    );
    let err = client.chat_stream(&bare_req(), &mut |_| {}).unwrap_err();
    assert!(matches!(err, ModelError::Protocol(_)), "{err:?}");
    server.join().unwrap();
}

/// A `ChatModel` with no streaming implementation still streams: the
/// default replays the blocking answer as events. This is what keeps
/// every provider (and every test mock) on one interface.
struct BlockingOnly;

impl zet_models::ChatModel for BlockingOnly {
    fn chat(&self, _req: &ChatRequest) -> Result<zet_models::ChatResponse, ModelError> {
        Ok(zet_models::ChatResponse {
            text: "done".to_string(),
            tool_calls: vec![ToolCall {
                id: "c1".to_string(),
                name: "grep".to_string(),
                arguments: "{}".to_string(),
            }],
        })
    }
}

impl ChatStream for BlockingOnly {}

#[test]
fn the_default_fallback_replays_the_blocking_answer() {
    let mut events = Vec::new();
    let res = BlockingOnly
        .chat_stream(&bare_req(), &mut |event| events.push(event))
        .unwrap();
    assert_eq!(res.text, "done");
    assert_eq!(
        events,
        vec![
            StreamEvent::TextDelta("done".to_string()),
            StreamEvent::ToolCallDelta {
                index: 0,
                id: Some("c1".to_string()),
                name: Some("grep".to_string()),
                arguments: "{}".to_string(),
            },
            StreamEvent::Done,
        ]
    );
}
