use std::io::{Read, Write};
use std::net::TcpListener;

/// Minimal /api/tags stub: serves fixed JSON to connections that speak
/// HTTP, ignoring bare probes (the autodetect TCP check connects and
/// closes without sending).
fn stub_tags(body: &'static str) -> (u16, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let h = std::thread::spawn(move || {
        // Exactly the connections one autodetect makes: bare probe + GET.
        for stream in listener.incoming().take(2).flatten() {
            let mut stream = stream;
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                match stream.read(&mut byte) {
                    Ok(0) => break,
                    Ok(_) => head.push(byte[0]),
                    Err(_) => break,
                }
                if head.len() > 65536 {
                    break;
                }
            }
            if head.is_empty() {
                continue;
            }
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    (port, h)
}

fn tags(names: &[&str]) -> String {
    let models: Vec<String> = names
        .iter()
        .map(|n| format!(r#"{{"name":{n:?}}}"#))
        .collect();
    format!(r#"{{"models":[{}]}}"#, models.join(","))
}

#[test]
fn autodetect_picks_preferred_model() {
    let body = tags(&["qwen3:1.7b", "llama3.2:1b", "nomic-embed-text"]);
    let leaked: &'static str = Box::leak(body.into_boxed_str());
    let (port, h) = stub_tags(leaked);
    let got = zet_models::keys::autodetect_local_at("127.0.0.1", port);
    assert_eq!(got.as_deref(), Some("local:llama3.2:1b"));
    h.join().unwrap();
}

#[test]
fn autodetect_falls_back_through_preferred_list() {
    let body = tags(&["qwen3:1.7b"]);
    let leaked: &'static str = Box::leak(body.into_boxed_str());
    let (port, h) = stub_tags(leaked);
    let got = zet_models::keys::autodetect_local_at("127.0.0.1", port);
    assert_eq!(got.as_deref(), Some("local:qwen3:1.7b"));
    h.join().unwrap();
}

#[test]
fn autodetect_none_when_closed_garbage_or_embed_only() {
    // Closed port: bind then drop for an (almost surely) shut port.
    let shut = TcpListener::bind("127.0.0.1:0").unwrap();
    let shut_port = shut.local_addr().unwrap().port();
    drop(shut);
    assert_eq!(
        zet_models::keys::autodetect_local_at("127.0.0.1", shut_port),
        None
    );
    let (port, h) = stub_tags("not json");
    assert_eq!(
        zet_models::keys::autodetect_local_at("127.0.0.1", port),
        None
    );
    h.join().unwrap();
    let body = tags(&["nomic-embed-text"]);
    let leaked: &'static str = Box::leak(body.into_boxed_str());
    let (port, h) = stub_tags(leaked);
    assert_eq!(
        zet_models::keys::autodetect_local_at("127.0.0.1", port),
        None
    );
    h.join().unwrap();
}

#[test]
fn autodetect_refuses_non_loopback() {
    assert_eq!(
        zet_models::keys::autodetect_local_at("93.184.216.34", 11434),
        None
    );
    assert_eq!(
        zet_models::keys::autodetect_local_at("10.0.0.8", 11434),
        None
    );
}
