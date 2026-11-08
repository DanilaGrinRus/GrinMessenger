// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the wallet console (`flintmsg-wallet-ui`):
//! the console serves its page, proxies `/api/v1/*` to a real wallet daemon
//! with the bearer token injected server-side, and forwards sends/contacts.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use flint_gateway::{wait_ready, Gateway, GatewayConfig};
use serde_json::{json, Value};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "flint-wallet-ui-it-{tag}-{}-{}",
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
        ..GatewayConfig::default()
    })
    .expect("relay");
    assert!(wait_ready(&relay, Duration::from_secs(5)));
    let url = relay.url();
    (relay, url)
}

struct Daemon {
    child: Child,
    url: String,
    token: String,
    token_path: PathBuf,
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
    let mut child = Command::new(env!("CARGO_BIN_EXE_flintmsg-walletd"))
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
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn flintmsg-walletd");
    let stdout = child.stdout.take().expect("daemon stdout");
    let mut line = String::new();
    BufReader::new(stdout)
        .read_line(&mut line)
        .expect("startup line");
    let startup: Value = serde_json::from_str(line.trim()).expect("startup JSON");
    let token_path = PathBuf::from(startup["token_file"].as_str().expect("token file"));
    let token = std::fs::read_to_string(&token_path)
        .expect("read token")
        .trim()
        .to_string();
    Daemon {
        child,
        url: startup["api"].as_str().expect("api").to_string(),
        token,
        token_path,
        address: startup["address"].as_str().expect("address").to_string(),
    }
}

struct Console {
    child: Child,
    url: String,
}

impl Drop for Console {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_console(api: &str, token_path: &Path) -> Console {
    let mut child = Command::new(env!("CARGO_BIN_EXE_flintmsg-wallet-ui"))
        .arg("--api")
        .arg(api)
        .arg("--token-file")
        .arg(token_path)
        .arg("--listen")
        .arg("127.0.0.1:0")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn flintmsg-wallet-ui");
    let stdout = child.stdout.take().expect("console stdout");
    let mut line = String::new();
    BufReader::new(stdout)
        .read_line(&mut line)
        .expect("startup line");
    let startup: Value = serde_json::from_str(line.trim()).expect("console startup JSON");
    Console {
        child,
        url: startup["url"]
            .as_str()
            .expect("console url")
            .trim_end_matches('/')
            .to_string(),
    }
}

fn http(base: &str, method: &str, path: &str, body: Option<&Value>) -> (u16, String) {
    let addr = base.strip_prefix("http://").expect("http base");
    let mut stream = TcpStream::connect(addr).unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    stream
        .set_read_timeout(Some(Duration::from_secs(40)))
        .unwrap();
    let payload = body
        .map(|value| serde_json::to_vec(value).unwrap())
        .unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        payload.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    stream.write_all(&payload).unwrap();
    stream.flush().unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("malformed response: {text:?}"));
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("malformed status: {head:?}"));
    (status, body.to_string())
}

fn json_http(base: &str, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    let (status, text) = http(base, method, path, body);
    let value = if text.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("json {text:?}: {e}"))
    };
    (status, value)
}

fn wait_daemon_ready(daemon: &Daemon) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let (status, body) = http_with_token(&daemon.url, "GET", "/v1/status", &daemon.token, None);
        if status == 200 && body["phase"] == "ready" {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "daemon not ready: {status} {body}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn http_with_token(
    base: &str,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&Value>,
) -> (u16, Value) {
    let addr = base.strip_prefix("http://").expect("http base");
    let mut stream = TcpStream::connect(addr).unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    stream
        .set_read_timeout(Some(Duration::from_secs(40)))
        .unwrap();
    let payload = body
        .map(|value| serde_json::to_vec(value).unwrap())
        .unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
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
        .unwrap_or_else(|| panic!("malformed status: {head:?}"));
    let value = if body.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(body).unwrap_or_else(|e| panic!("json {body:?}: {e}"))
    };
    (status, value)
}

#[test]
fn console_serves_page_and_proxies_the_wallet_api() {
    let dir = temp_dir("proxy");
    let (_relay, relay_url) = start_relay(&dir);
    let alice = spawn_daemon(&dir, &relay_url, "alice");
    let bob = spawn_daemon(&dir, &relay_url, "bob");
    wait_daemon_ready(&alice);
    wait_daemon_ready(&bob);
    let console = spawn_console(&alice.url, &alice.token_path);

    let (status, page) = http(&console.url, "GET", "/", None);
    assert_eq!(status, 200, "console page");
    assert!(page.contains("GrinMessenger Wallet Console"), "page title");
    assert!(page.contains("/api/v1/status"), "page talks to the proxy");
    assert!(
        !page.contains(&alice.token),
        "token must never be embedded in the page"
    );

    let (status, meta) = json_http(&console.url, "GET", "/api/meta", None);
    assert_eq!(status, 200);
    assert_eq!(meta["api"], alice.url);

    let (status, status_body) = json_http(&console.url, "GET", "/api/v1/status", None);
    assert_eq!(status, 200, "proxied status: {status_body}");
    assert_eq!(status_body["address"], alice.address);
    assert!(
        !status_body.to_string().contains(&alice.token),
        "token must not leak through the proxy"
    );

    let (status, caps) = json_http(&console.url, "GET", "/api/v1/capabilities", None);
    assert_eq!(status, 200);
    assert!(caps["kinds"]
        .as_array()
        .unwrap()
        .iter()
        .any(|kind| kind == "slatepack"));

    // Add a contact through the console proxy.
    let (status, body) = json_http(
        &console.url,
        "POST",
        "/api/v1/contacts",
        Some(&json!({"address": bob.address, "name": "Bob"})),
    );
    assert_eq!(status, 200, "add contact: {body}");
    let (_, contacts) = json_http(&console.url, "GET", "/api/v1/contacts", None);
    assert_eq!(contacts["contacts"].as_array().unwrap().len(), 1);

    // Send text through the console; Bob receives it via the real relay.
    let (status, body) = json_http(
        &console.url,
        "POST",
        "/api/v1/send",
        Some(&json!({"to": bob.address, "kind": "text", "text": "hello from the console"})),
    );
    assert_eq!(status, 200, "console send: {body}");
    assert_eq!(body["message"]["direction"], "outgoing");

    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let (_, messages) = http_with_token(
            &bob.url,
            "GET",
            "/v1/messages?after=0&limit=100",
            &bob.token,
            None,
        );
        if messages["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["text"] == "hello from the console")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Bob did not receive the console message"
        );
        std::thread::sleep(Duration::from_millis(200));
    }

    // Outgoing message is visible through the console too.
    let (_, messages) = json_http(
        &console.url,
        "GET",
        "/api/v1/messages?after=0&limit=100",
        None,
    );
    assert!(messages["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["text"] == "hello from the console"));

    // Wallet daemon errors are forwarded with their status.
    let (status, _) = json_http(&console.url, "GET", "/api/v1/nope", None);
    assert_eq!(status, 404);
    let (status, _) = http(&console.url, "GET", "/nope", None);
    assert_eq!(status, 404);

    let _ = console;
    let _ = bob;
    let _ = alice;
    std::fs::remove_dir_all(dir).ok();
}

fn raw_request(
    base: &str,
    method: &str,
    path: &str,
    host: Option<&str>,
    origin: Option<&str>,
    content_type: Option<&str>,
    body: &str,
) -> (u16, String) {
    let addr = base.strip_prefix("http://").expect("http base");
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let mut head = String::new();
    if let Some(host) = host {
        head.push_str(&format!("Host: {host}\r\n"));
    }
    if let Some(origin) = origin {
        head.push_str(&format!("Origin: {origin}\r\n"));
    }
    if let Some(content_type) = content_type {
        head.push_str(&format!("Content-Type: {content_type}\r\n"));
    }
    let request = format!(
        "{method} {path} HTTP/1.1\r\n{head}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    stream.flush().unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    let status: u16 = text
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("malformed response: {text:?}"));
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default();
    (status, body)
}

#[test]
fn console_is_guarded_against_dns_rebinding_and_csrf() {
    let dir = temp_dir("guard");
    let (_relay, relay_url) = start_relay(&dir);
    let daemon = spawn_daemon(&dir, &relay_url, "solo");
    wait_daemon_ready(&daemon);
    let console = spawn_console(&daemon.url, &daemon.token_path);
    let port = console.url.rsplit(':').next().unwrap();
    let host = format!("127.0.0.1:{port}");

    // Foreign / missing Host means DNS rebinding; reject before routing.
    let (status, _) = raw_request(
        &console.url,
        "GET",
        "/",
        Some(&format!("evil.example:{port}")),
        None,
        None,
        "",
    );
    assert_eq!(status, 403, "rebinding Host must be rejected");
    let (status, _) = raw_request(&console.url, "GET", "/api/meta", None, None, None, "");
    assert_eq!(status, 403, "missing Host must be rejected");
    let (status, _) = raw_request(
        &console.url,
        "GET",
        "/api/v1/status",
        Some(&host),
        None,
        None,
        "",
    );
    assert_eq!(status, 200, "same-origin GET works");

    // Cross-site Origin is rejected even though the proxy would inject the token.
    let body = format!(
        "{{\"to\":\"{}\",\"kind\":\"text\",\"text\":\"x\"}}",
        daemon.address
    );
    let (status, _) = raw_request(
        &console.url,
        "POST",
        "/api/v1/send",
        Some(&host),
        Some("http://evil.example"),
        Some("application/json"),
        &body,
    );
    assert_eq!(status, 403, "foreign Origin must be rejected");

    // CORS-simple text/plain POST never reaches the proxy.
    let (status, _) = raw_request(
        &console.url,
        "POST",
        "/api/v1/send",
        Some(&host),
        None,
        Some("text/plain"),
        &body,
    );
    assert_eq!(status, 415, "text/plain must be rejected");

    // The legitimate same-origin JSON request is forwarded.
    let (status, _) = raw_request(
        &console.url,
        "POST",
        "/api/v1/send",
        Some(&host),
        Some(&format!("http://{host}")),
        Some("application/json"),
        &body,
    );
    assert_eq!(status, 200, "same-origin send must be proxied");

    let _ = console;
    let _ = daemon;
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn console_refuses_unsafe_configuration() {
    let dir = temp_dir("config");
    let token_file = dir.join("short-token");
    std::fs::write(&token_file, "short").unwrap();

    let non_loopback = Command::new(env!("CARGO_BIN_EXE_flintmsg-wallet-ui"))
        .arg("--api")
        .arg("http://10.0.0.5:1234")
        .arg("--token-file")
        .arg(&token_file)
        .output()
        .expect("run");
    assert!(!non_loopback.status.success());
    assert!(String::from_utf8_lossy(&non_loopback.stderr).contains("refusing a non-loopback"));

    let weak_token = Command::new(env!("CARGO_BIN_EXE_flintmsg-wallet-ui"))
        .arg("--api")
        .arg("http://127.0.0.1:1234")
        .arg("--token-file")
        .arg(&token_file)
        .output()
        .expect("run");
    assert!(!weak_token.status.success());
    assert!(String::from_utf8_lossy(&weak_token.stderr).contains("too short"));

    let missing = Command::new(env!("CARGO_BIN_EXE_flintmsg-wallet-ui"))
        .arg("--api")
        .arg("http://127.0.0.1:1234")
        .arg("--token-file")
        .arg(dir.join("missing-token"))
        .output()
        .expect("run");
    assert!(!missing.status.success());

    std::fs::remove_dir_all(dir).ok();
}
