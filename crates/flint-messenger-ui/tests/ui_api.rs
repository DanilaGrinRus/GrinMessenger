// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! Integration tests for the local web UI: one real `flint-relay` gateway
//! (in-process), one real `flintmsg-ui` process and two identities.
//!
//! The tests speak raw HTTP to the UI and use the messenger library directly for
//! the peer, so the whole path "library peer ↔ relay ↔ UI" is exercised.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use flint_gateway::gateway::{Gateway, GatewayConfig};
use flint_messenger::transport::GatewayTransport;
use flint_messenger::{Address, Identity, MessageKind, Messenger, DEFAULT_TTL_SECS};
use serde_json::Value;

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind free port");
    let port = listener.local_addr().expect("port").port();
    drop(listener);
    port
}

fn unique_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "flintmsg-ui-test-{name}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn http(port: u16, method: &str, path: &str, body: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect ui");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).expect("write request");
    stream.flush().ok();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read response");
    let text = String::from_utf8_lossy(&raw).to_string();
    let status = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let response_body = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default();
    (status, response_body)
}

fn json_state(port: u16) -> Value {
    let (status, body) = http(port, "GET", "/api/state", "");
    assert_eq!(status, 200, "state status; body={body}");
    serde_json::from_str(&body).expect("state json")
}

fn wait_until(deadline: Instant, mut predicate: impl FnMut() -> bool) -> bool {
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    false
}

struct UiProcess {
    child: Child,
}

impl Drop for UiProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_ui(port: u16, data_dir: &PathBuf, relay_url: &str) -> UiProcess {
    let child = Command::new(env!("CARGO_BIN_EXE_flintmsg-ui"))
        .args([
            "--data",
            data_dir.to_str().expect("data path"),
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--relay",
            relay_url,
            "--plain",
            "--poll-secs",
            "1",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn flintmsg-ui");
    UiProcess { child }
}

fn relay() -> Gateway {
    Gateway::start(GatewayConfig {
        bind: "127.0.0.1:0".parse().expect("bind"),
        require_signed_ack: true,
        require_pow: true,
        ..GatewayConfig::default()
    })
    .expect("relay")
}

#[test]
fn ui_chat_roundtrip_and_contacts() {
    let gateway = relay();
    let data_dir = unique_dir("chat");
    let port = free_port();
    let _ui = spawn_ui(port, &data_dir, &gateway.url());

    // Wait for the UI transport to become ready and the identity to appear.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut state = json_state(port);
    assert!(
        wait_until(deadline, || {
            state = json_state(port);
            state["status"]["phase"] == "ready"
        }),
        "UI transport did not become ready: {state}"
    );
    let alice_address = state["identity"]["address"]
        .as_str()
        .expect("address")
        .to_string();
    assert!(
        alice_address.starts_with("tgrin1"),
        "unexpected address {alice_address}"
    );
    assert_eq!(
        state["mode"]["kind"], "relay",
        "relay mode expected: {state}"
    );

    // The HTML page is served by the same process.
    let (page_status, page) = http(port, "GET", "/", "");
    assert_eq!(page_status, 200);
    assert!(page.contains("GrinMessenger"));
    assert!(page.contains("Envelope inspector"));

    // Peer (library side) sends Alice a message through the real relay.
    let bob = Identity::from_seed([9u8; 32], "tgrin").expect("bob");
    let bob_address: Address = bob.address().clone();
    let alice: Address = alice_address.parse().expect("alice address");
    let bob_messenger = Messenger::new(GatewayTransport::new(&gateway.url()).expect("transport"));
    let sent = bob_messenger
        .send(
            &bob,
            &alice,
            MessageKind::Text {
                text: "hello from the library peer".into(),
            },
            DEFAULT_TTL_SECS,
        )
        .expect("bob send");

    // The UI poll loop must fetch, open and display it.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut received = false;
    let mut final_state = state.clone();
    let found = wait_until(deadline, || {
        final_state = json_state(port);
        if let Some(messages) = final_state["messages"].as_array() {
            received = messages.iter().any(|message| {
                message["id"] == sent.id
                    && message["direction"] == "in"
                    && message["kind"] == "text"
                    && message["text"] == "hello from the library peer"
            });
        }
        received
    });
    assert!(
        found,
        "UI did not display the incoming message: {final_state}"
    );
    assert!(
        final_state["status"]["last_error"].is_null(),
        "UI poll reported an error: {final_state}"
    );

    // Sending from the UI to the library peer works end to end.
    let (send_status, send_body) = http(
        port,
        "POST",
        "/api/send",
        &format!(
            "{{\"to\":\"{}\",\"text\":\"reply from the web UI\"}}",
            bob_address
        ),
    );
    assert_eq!(send_status, 200, "send failed: {send_body}");
    let send_json: Value = serde_json::from_str(&send_body).expect("send json");
    assert_eq!(send_json["ok"], true);
    let reply_id = send_json["id"].as_str().expect("id").to_string();
    let fetched = bob_messenger.fetch_open(&bob, 20).expect("bob fetch");
    assert!(
        fetched
            .opened
            .iter()
            .any(|opened| opened.id == reply_id
                && matches!(&opened.kind, MessageKind::Text { text } if text == "reply from the web UI")),
        "reply not delivered to the library peer"
    );

    // Contacts API.
    let (contact_status, _) = http(
        port,
        "POST",
        "/api/contacts",
        &format!("{{\"name\":\"Bob\",\"address\":\"{}\"}}", bob_address),
    );
    assert_eq!(contact_status, 200);
    let state = json_state(port);
    assert!(
        state["contacts"]
            .as_array()
            .expect("contacts")
            .iter()
            .any(
                |contact| contact["name"] == "Bob" && contact["address"] == bob_address.to_string()
            ),
        "contact missing: {state}"
    );

    // Export returns the transcript with both messages.
    let (export_status, export_body) = http(port, "GET", "/api/export", "");
    assert_eq!(export_status, 200);
    let export: Value = serde_json::from_str(&export_body).expect("export json");
    assert_eq!(export["format"], "grinmessenger-transcript-v1");
    assert_eq!(
        export["messages"].as_array().map(Vec::len),
        Some(2),
        "unexpected transcript: {export}"
    );

    // Remove the contact again.
    let (remove_status, _) = http(
        port,
        "POST",
        "/api/contacts/remove",
        &format!("{{\"address\":\"{}\"}}", bob_address),
    );
    assert_eq!(remove_status, 200);
    assert!(
        json_state(port)["contacts"]
            .as_array()
            .map(Vec::is_empty)
            .unwrap_or(false),
        "contact was not removed"
    );

    std::fs::remove_dir_all(&data_dir).ok();
}

#[test]
fn invalid_requests_are_rejected() {
    let gateway = relay();
    let data_dir = unique_dir("invalid");
    let port = free_port();
    let _ui = spawn_ui(port, &data_dir, &gateway.url());

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut ready = false;
    wait_until(deadline, || {
        ready = json_state(port)["status"]["phase"] == "ready";
        ready
    });
    assert!(ready, "UI not ready");

    // Bad JSON.
    let (status, body) = http(port, "POST", "/api/send", "{not json");
    assert_eq!(status, 400, "bad json must be rejected: {body}");
    // Bad address.
    let (status, body) = http(
        port,
        "POST",
        "/api/send",
        "{\"to\":\"nope\",\"text\":\"x\"}",
    );
    assert_eq!(status, 400, "bad address must be rejected: {body}");
    // Empty text.
    let (status, body) = http(
        port,
        "POST",
        "/api/send",
        "{\"to\":\"tgrin1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq\",\"text\":\"\"}",
    );
    assert_eq!(status, 400, "empty text must be rejected: {body}");
    // Unknown endpoint.
    let (status, _) = http(port, "GET", "/api/nope", "");
    assert_eq!(status, 404);
    // Trailing slash is not the root document.
    let (status, _) = http(port, "GET", "/index.html", "");
    assert_eq!(status, 200);

    std::fs::remove_dir_all(&data_dir).ok();
}
