// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end wallet integration tests: two real `flintmsg-walletd` processes
//! talking through a real `flint-relay`, exactly like a wallet application would.
//!
//! Run with `cargo test -p flint-messenger-wallet --test wallet_api`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use flint_gateway::{wait_ready, Gateway, GatewayConfig};
use serde_json::{json, Value};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "flint-wallet-it-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn start_relay(dir: &Path) -> (Gateway, String) {
    let relay = Gateway::start(GatewayConfig {
        bind: "127.0.0.1:0".parse().unwrap(),
        store_dir: Some(dir.join("relay-store")),
        require_signed_ack: true,
        require_pow: true,
        max_payload: 64 * 1024,
        ..GatewayConfig::default()
    })
    .expect("relay");
    assert!(wait_ready(&relay, Duration::from_secs(5)), "relay health");
    let url = relay.url();
    (relay, url)
}

struct Daemon {
    child: Child,
    url: String,
    token: String,
    address: String,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_daemon(dir: &Path, relay_url: &str, tag: &str) -> Daemon {
    let data = dir.join(format!("data-{tag}"));
    let identity = data.join("identity.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_flintmsg-walletd"));
    command
        .arg("--data")
        .arg(&data)
        .arg("--identity")
        .arg(&identity)
        .arg("--generate")
        .arg("--network")
        .arg("tgrin")
        .arg("--relay")
        .arg(relay_url)
        .arg("--plain")
        .arg("--listen")
        .arg("127.0.0.1:0")
        .arg("--poll-secs")
        .arg("1")
        .arg("--limit")
        .arg("50")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = command.spawn().expect("spawn flintmsg-walletd");
    let stdout = child.stdout.take().expect("daemon stdout");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read startup line");
    let startup: Value =
        serde_json::from_str(line.trim()).unwrap_or_else(|e| panic!("startup JSON {line:?}: {e}"));
    assert_eq!(startup["event"], "ready", "startup: {startup}");
    let url = startup["api"].as_str().expect("api url").to_string();
    let token_path = startup["token_file"].as_str().expect("token_file");
    let token = std::fs::read_to_string(token_path)
        .expect("read token file")
        .trim()
        .to_string();
    let address = startup["address"].as_str().expect("address").to_string();
    Daemon {
        child,
        url,
        token,
        address,
    }
}

fn http(
    base: &str,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&Value>,
) -> (u16, Value) {
    let addr = base.strip_prefix("http://").expect("http base");
    let mut stream = TcpStream::connect(addr).unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let payload = body
        .map(|value| serde_json::to_vec(value).unwrap())
        .unwrap_or_default();
    let auth = token
        .map(|token| format!("Authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{auth}\r\n",
        payload.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    stream.write_all(&payload).unwrap();
    stream.flush().unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("malformed response: {text:?}"));
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("malformed status line: {head:?}"));
    let json = if body.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(body).unwrap_or_else(|e| panic!("response body {body:?}: {e}"))
    };
    (status, json)
}

fn wait_daemon_ready(daemon: &Daemon, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let (status, body) = http(&daemon.url, "GET", "/v1/status", Some(&daemon.token), None);
        if status == 200 && body["phase"] == "ready" {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "daemon did not become ready: {status} {body}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn wait_incoming(daemon: &Daemon, from: &str, kind: &str, timeout: Duration) -> Value {
    let deadline = Instant::now() + timeout;
    loop {
        let (status, body) = http(
            &daemon.url,
            "GET",
            "/v1/messages?after=0&limit=200",
            Some(&daemon.token),
            None,
        );
        assert_eq!(status, 200, "messages: {body}");
        if let Some(messages) = body["messages"].as_array() {
            if let Some(message) = messages.iter().find(|message| {
                message["direction"] == "incoming"
                    && message["peer"] == from
                    && message["kind"] == kind
            }) {
                return message.clone();
            }
        }
        assert!(
            Instant::now() < deadline,
            "no incoming {kind} from {from}: {body}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn wallet_daemons_exchange_text_and_slatepack() {
    let dir = temp_dir("roundtrip");
    let (_relay, relay_url) = start_relay(&dir);
    let alice = spawn_daemon(&dir, &relay_url, "alice");
    let bob = spawn_daemon(&dir, &relay_url, "bob");
    wait_daemon_ready(&alice, Duration::from_secs(20));
    wait_daemon_ready(&bob, Duration::from_secs(20));

    // Wallet address book: add Bob to Alice's contacts.
    let (status, body) = http(
        &alice.url,
        "POST",
        "/v1/contacts",
        Some(&alice.token),
        Some(&json!({"address": bob.address, "name": "Bob"})),
    );
    assert_eq!(status, 200, "add contact: {body}");
    assert_eq!(body["contact"]["name"], "Bob");
    let (status, body) = http(&alice.url, "GET", "/v1/contacts", Some(&alice.token), None);
    assert_eq!(status, 200);
    assert_eq!(body["contacts"].as_array().unwrap().len(), 1);

    // Alice -> Bob text.
    let (status, body) = http(
        &alice.url,
        "POST",
        "/v1/send",
        Some(&alice.token),
        Some(&json!({"to": bob.address, "kind": "text", "text": "hello bob"})),
    );
    assert_eq!(status, 200, "send text: {body}");
    assert_eq!(body["message"]["direction"], "outgoing");
    let text = wait_incoming(&bob, &alice.address, "text", Duration::from_secs(15));
    assert_eq!(text["text"], "hello bob");
    assert_eq!(text["direction"], "incoming");
    assert!(text["seq"].as_u64().unwrap() > 0);

    // Unread counters and read markers.
    let (status, unread) = http(&bob.url, "GET", "/v1/unread", Some(&bob.token), None);
    assert_eq!(status, 200);
    assert!(unread["total"].as_u64().unwrap() >= 1);
    let (status, body) = http(
        &bob.url,
        "POST",
        "/v1/read",
        Some(&bob.token),
        Some(&json!({"peer": alice.address})),
    );
    assert_eq!(status, 200, "mark read: {body}");
    let (status, unread) = http(&bob.url, "GET", "/v1/unread", Some(&bob.token), None);
    assert_eq!(status, 200);
    assert_eq!(
        unread["peers"][&alice.address].as_u64().unwrap_or(0),
        0,
        "read marker: {unread}"
    );

    // Events cursor: the delivered message must be visible as a message event.
    let (status, events) = http(
        &bob.url,
        "GET",
        "/v1/events?since=0&wait=2",
        Some(&bob.token),
        None,
    );
    assert_eq!(status, 200, "events: {events}");
    let events = events["events"].as_array().unwrap();
    assert!(
        events.iter().any(|event| event["type"] == "message"
            && event["message"]["kind"] == "text"
            && event["message"]["text"] == "hello bob"
            && event["message"]["seq"].as_u64().unwrap_or(0) > 0),
        "missing message event with a cursor sequence: {events:?}"
    );

    // Bob -> Alice Slatepack armor (opaque for the messenger).
    let armor = "BEGINSLATEPACK.5XQXXX.END SLATEPACK";
    let (status, body) = http(
        &bob.url,
        "POST",
        "/v1/send",
        Some(&bob.token),
        Some(&json!({"to": alice.address, "kind": "slatepack", "armor": armor})),
    );
    assert_eq!(status, 200, "send slatepack: {body}");
    let slatepack = wait_incoming(&alice, &bob.address, "slatepack", Duration::from_secs(15));
    assert_eq!(slatepack["armor"], armor);
    assert_eq!(slatepack["encrypted"], true);

    // Delete the contact.
    let (status, body) = http(
        &alice.url,
        "DELETE",
        &format!("/v1/contacts/{}", bob.address),
        Some(&alice.token),
        None,
    );
    assert_eq!(status, 200, "delete contact: {body}");
    assert_eq!(body["removed"], true);

    let _ = alice;
    let _ = bob;
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn api_rejects_bad_auth_and_invalid_requests() {
    let dir = temp_dir("validation");
    let (_relay, relay_url) = start_relay(&dir);
    let daemon = spawn_daemon(&dir, &relay_url, "solo");
    wait_daemon_ready(&daemon, Duration::from_secs(20));

    // Health is the only unauthenticated endpoint.
    let (status, body) = http(&daemon.url, "GET", "/v1/health", None, None);
    assert_eq!(status, 200, "health: {body}");
    assert_eq!(body["ok"], true);

    let (status, _) = http(&daemon.url, "GET", "/v1/status", None, None);
    assert_eq!(status, 401, "missing token");
    let (status, _) = http(&daemon.url, "GET", "/v1/status", Some("deadbeef"), None);
    assert_eq!(status, 401, "bad token");
    let (status, _) = http(&daemon.url, "GET", "/v1/status", Some(&daemon.token), None);
    assert_eq!(status, 200);

    let (status, body) = http(
        &daemon.url,
        "POST",
        "/v1/send",
        Some(&daemon.token),
        Some(&json!({"to": "not-an-address", "text": "hi"})),
    );
    assert_eq!(status, 400, "invalid address: {body}");

    let (status, body) = http(
        &daemon.url,
        "POST",
        "/v1/send",
        Some(&daemon.token),
        Some(&json!({"to": daemon.address, "kind": "slatepack", "armor": ""})),
    );
    assert_eq!(status, 400, "empty slatepack: {body}");
    assert!(body["error"].as_str().unwrap().contains("armor"));

    let (status, body) = http(
        &daemon.url,
        "POST",
        "/v1/send",
        Some(&daemon.token),
        Some(&json!({"to": daemon.address, "kind": "text"})),
    );
    assert_eq!(status, 400, "missing text: {body}");

    let (status, body) = http(
        &daemon.url,
        "POST",
        "/v1/contacts",
        Some(&daemon.token),
        Some(&json!({"address": "bad", "name": "x"})),
    );
    assert_eq!(status, 400, "bad contact: {body}");

    let (status, body) = http(
        &daemon.url,
        "POST",
        "/v1/send",
        Some(&daemon.token),
        Some(&json!({"to": daemon.address, "kind": "gold"})),
    );
    assert_eq!(status, 400, "unknown kind: {body}");

    let (status, _) = http(&daemon.url, "GET", "/v1/nope", Some(&daemon.token), None);
    assert_eq!(status, 404);

    // The token file exists, is readable and looks like 32 random bytes in hex.
    let token_file = dir.join("data-solo").join("api-token");
    let token = std::fs::read_to_string(&token_file).expect("token file");
    assert_eq!(token.trim().len(), 64);
    assert!(token.trim().chars().all(|c| c.is_ascii_hexdigit()));

    let _ = daemon;
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn identity_subcommand_derives_address_without_printing_seed() {
    let dir = temp_dir("identity");
    let seed = [42u8; 32];
    let seed_file = dir.join("seed.hex");
    std::fs::write(&seed_file, data_encoding::HEXLOWER.encode(&seed)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_flintmsg-walletd"))
        .arg("identity")
        .arg("--seed-file")
        .arg(&seed_file)
        .arg("--network")
        .arg("tgrin")
        .output()
        .expect("run identity");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: Value = serde_json::from_str(stdout.trim()).expect("identity JSON");
    let expected = flint_messenger::Identity::from_seed(seed, "tgrin").unwrap();
    assert_eq!(value["address"], expected.address().to_string());
    assert_eq!(value["onion"], expected.address().onion());
    assert!(!stdout.contains("seed_hex"), "seed must not be printed");
    std::fs::remove_dir_all(dir).ok();
}
