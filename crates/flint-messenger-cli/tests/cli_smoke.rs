// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! End-to-end smoke test of the `flintmsg` binary itself: keygen/address, send
//! through a real loopback `flint-gateway` with `--plain`, fetch + signed ACK,
//! and the refusal to use `--plain` for a `.onion` destination.
//!
//! No Tor network is required; the direct-Arti path is covered by the ignored
//! `flint-tor-arti` live test and by manual/evidence runs.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use flint_gateway::gateway::{Gateway, GatewayConfig};
use flint_messenger::transport::GatewayTransport;
use flint_messenger::Transport;

fn unique_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "flintmsg-cli-{name}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn flintmsg(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flintmsg"))
        .args(args)
        .output()
        .expect("spawn flintmsg")
}

fn stdout_json(output: &Output) -> serde_json::Value {
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with('{'))
        .unwrap_or_else(|| {
            panic!(
                "no JSON output: stdout={text:?} stderr={:?}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
    serde_json::from_str(line).expect("valid JSON line")
}

fn keygen(identity: &Path, network: &str) {
    let output = flintmsg(&[
        "keygen",
        "--out",
        identity.to_str().expect("path"),
        "--network",
        network,
    ]);
    assert!(
        output.status.success(),
        "keygen failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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
fn cli_keygen_send_and_fetch_through_plain_relay() {
    let dir = unique_dir("flow");
    let alice_path = dir.join("alice.json");
    let bob_path = dir.join("bob.json");
    keygen(&alice_path, "tgrin");
    keygen(&bob_path, "tgrin");

    let alice = stdout_json(&flintmsg(&[
        "address",
        "--identity",
        alice_path.to_str().expect("path"),
    ]));
    assert_eq!(alice["event"], "address");
    assert!(alice["address"]
        .as_str()
        .expect("address")
        .starts_with("tgrin1"));
    assert_eq!(alice["onion"].as_str().expect("onion").len(), 56);

    let bob = stdout_json(&flintmsg(&[
        "address",
        "--identity",
        bob_path.to_str().expect("path"),
    ]));
    let bob_address = bob["address"].as_str().expect("bob address").to_string();

    let gateway = relay();
    let send = flintmsg(&[
        "send",
        "--identity",
        alice_path.to_str().expect("path"),
        "--to",
        &bob_address,
        "--text",
        "cli hello",
        "--relay",
        &gateway.url(),
        "--plain",
    ]);
    assert!(
        send.status.success(),
        "send failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&send.stdout),
        String::from_utf8_lossy(&send.stderr)
    );
    let sent = stdout_json(&send);
    assert_eq!(sent["event"], "sent");
    assert_eq!(sent["via"], "relay");

    // The relay stores an opaque envelope; fetch it as Bob through the CLI.
    let fetch = Command::new(env!("CARGO_BIN_EXE_flintmsg"))
        .args([
            "fetch",
            "--identity",
            bob_path.to_str().expect("path"),
            "--source",
            &gateway.url(),
            "--plain",
        ])
        .output()
        .expect("spawn fetch");
    assert!(
        fetch.status.success(),
        "fetch failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&fetch.stdout),
        String::from_utf8_lossy(&fetch.stderr)
    );
    let text = String::from_utf8_lossy(&fetch.stdout);
    assert!(text.contains("\"event\":\"message\""), "no message: {text}");
    assert!(text.contains("cli hello"), "payload missing: {text}");
    assert!(text.contains("\"event\":\"acked\""), "no ack: {text}");

    // Signed ACK removed the message; a second fetch returns nothing.
    let transport = GatewayTransport::new(&gateway.url()).expect("transport");
    let remaining = transport
        .fetch(
            &bob_address.parse().expect("address"),
            flint_messenger::envelope::unix_now(),
            10,
        )
        .expect("relay fetch");
    assert!(remaining.is_empty(), "message must be acked away");

    drop(gateway);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn keygen_refuses_to_overwrite_without_force() {
    let dir = unique_dir("overwrite");
    let identity = dir.join("identity.json");
    keygen(&identity, "tgrin");
    let output = flintmsg(&[
        "keygen",
        "--out",
        identity.to_str().expect("path"),
        "--network",
        "tgrin",
    ]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--force"),
        "stderr: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn plain_mode_refuses_onion_target() {
    let dir = unique_dir("onion");
    let identity = dir.join("identity.json");
    keygen(&identity, "tgrin");
    let own = stdout_json(&flintmsg(&[
        "address",
        "--identity",
        identity.to_str().expect("path"),
    ]));
    let to = own["address"].as_str().expect("address").to_string();
    let output = flintmsg(&[
        "send",
        "--identity",
        identity.to_str().expect("path"),
        "--to",
        &to,
        "--text",
        "x",
        "--relay",
        "http://example.invalid.onion",
        "--plain",
    ]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(".onion"),
        "stderr: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(&dir).ok();
}
