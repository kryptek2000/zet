use std::io::{Read, Write};
use std::net::TcpListener;
use zet_models::list_models;

fn stub_models(
    listener: TcpListener,
    body: &'static str,
    status: u16,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if stream.read_exact(&mut byte).is_err() || head.len() > 65536 {
                break;
            }
            head.push(byte[0]);
        }
        let head_text = String::from_utf8_lossy(&head).into_owned();
        assert!(head_text.contains("GET /models"), "wrong path: {head_text}");
        assert!(
            head_text
                .to_lowercase()
                .contains("authorization: bearer test-key"),
            "missing auth: {head_text}"
        );
        let reply = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(reply.as_bytes()).unwrap();
    })
}

fn base(listener: &TcpListener) -> String {
    format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port())
}

#[test]
fn lists_ids_from_openai_shape() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port_base = base(&listener);
    let h = stub_models(
        listener,
        r#"{"data":[{"id":"kimi-k2.7-code"},{"id":"glm-5.3"},{"id":"other"}]}"#,
        200,
    );
    let ids = list_models(&port_base, "test-key").unwrap();
    assert_eq!(ids, vec!["kimi-k2.7-code", "glm-5.3", "other"]);
    h.join().unwrap();
}

#[test]
fn auth_failure_surfaces_not_silence() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port_base = base(&listener);
    let h = stub_models(listener, r#"{"error":"bad key"}"#, 401);
    let err = list_models(&port_base, "test-key").unwrap_err().to_string();
    assert!(err.contains("401") || err.contains("auth"), "{err}");
    h.join().unwrap();
}

#[test]
fn free_filter_is_suffix_match() {
    assert!(zet_models::is_free_model("poolside/laguna-s-2.1:free"));
    assert!(!zet_models::is_free_model("kimi-k2.7-code"));
}
