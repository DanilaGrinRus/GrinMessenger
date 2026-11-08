// SPDX-License-Identifier: MIT OR Apache-2.0
//! Live local-relay smoke: the real `flint-relay` process, opaque envelopes,
//! broken-envelope quarantine and restart-style skip.
//!
//! The binary is located through Cargo's `CARGO_BIN_EXE_<name>`, so no build
//! path is hard-coded:
//!
//!   cargo test -p flint-gateway --test relay_live -- --ignored --test-threads=1

use std::collections::HashSet;
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use flint_messenger::transport::GatewayTransport;
use flint_messenger::{Envelope, Identity, MessageKind, Messenger, Transport, DEFAULT_TTL_SECS};

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind free port")
        .local_addr()
        .expect("local addr")
        .port()
}

fn relay_exe() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_flint-relay"))
}

fn wait_ready(port: u16, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("flint-relay did not become ready on port {port}");
}

struct Relay {
    child: Child,
    dir: std::path::PathBuf,
}

impl Drop for Relay {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn spawn_relay() -> (Relay, String) {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let dir = std::env::temp_dir().join(format!("flint-relay-live-{stamp}"));
    std::fs::create_dir_all(&dir).expect("relay store");
    let port = free_port();
    let child = Command::new(relay_exe())
        .args([
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--store",
            &dir.to_string_lossy(),
            "--max-per",
            "200",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn flint-relay");
    wait_ready(port, Duration::from_secs(8));
    (Relay { child, dir }, format!("http://127.0.0.1:{port}"))
}

#[test]
#[ignore = "spawns the real flint-relay binary"]
fn local_relay_quarantines_broken_envelopes_and_restart_skips_them() {
    let (_relay, url) = spawn_relay();
    let transport = GatewayTransport::new(&url).expect("gateway transport");
    let messenger = Messenger::new(transport);
    let alice = Identity::generate("tgrin").expect("alice");
    let bob = Identity::generate("tgrin").expect("bob");
    let good = messenger
        .send(
            &alice,
            bob.address(),
            MessageKind::Text {
                text: "live relay good".into(),
            },
            DEFAULT_TTL_SECS,
        )
        .expect("create message");
    let good_id = good.id.clone();

    let raw = GatewayTransport::new(&url).expect("raw transport");
    for n in 0..40 {
        // A legacy v1 envelope gets a valid UUID and PoW; corrupt only the
        // outer signature (v1 still verifies it) so the relay accepts it and
        // the recipient reports it as failed. v2 cannot be corrupted this way
        // because the sender proof is inside the AEAD-protected payload.
        let mut bad = Envelope::seal_legacy_v1(
            &alice,
            bob.address(),
            MessageKind::Text {
                text: format!("broken {n}"),
            },
            DEFAULT_TTL_SECS,
            flint_messenger::envelope::unix_now(),
        )
        .expect("seal broken");
        bad.signature[0] ^= 1;
        raw.deliver(&bad).expect("deliver broken");
    }

    let batch = messenger.fetch_open(&bob, 1).expect("fetch live relay");
    assert_eq!(batch.opened.len(), 1, "good message must survive");
    assert_eq!(batch.opened[0].id, good_id);
    assert_eq!(batch.failed.len(), 40, "all broken envelopes reported");

    let skip: HashSet<String> = batch.failed.iter().map(|f| f.id.clone()).collect();
    let restarted = Messenger::new(GatewayTransport::new(&url).expect("new transport"));
    let second = restarted
        .fetch_open_skipping(&bob, 1, &skip)
        .expect("fetch after restart");
    assert_eq!(second.opened.len(), 1, "good message after restart");
    assert!(second.failed.is_empty(), "quarantined ids must be skipped");

    let mut ack = second
        .opened
        .iter()
        .map(|m| m.id.clone())
        .collect::<Vec<_>>();
    ack.extend(skip);
    restarted.ack_signed(&bob, &ack).expect("signed ack");
}
