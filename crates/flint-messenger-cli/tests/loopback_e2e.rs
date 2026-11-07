// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! Loopback end-to-end tests for the extracted messenger stack.
//!
//! These run a real `flint-gateway` relay over TCP loopback and exercise the
//! protocol with two independent identities: delivery, signed ACK, at-least-once
//! deduplication, tamper quarantine, PoW policy, persistence across a relay
//! restart, and Arti onion-address derivation. No Tor network is required.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use flint_gateway::gateway::{Gateway, GatewayConfig};
use flint_messenger::transport::GatewayTransport;
use flint_messenger::{Envelope, Identity, MessageKind, Messenger, Transport, DEFAULT_TTL_SECS};

fn start_relay(store: Option<PathBuf>, require_pow: bool) -> Gateway {
    Gateway::start(GatewayConfig {
        bind: "127.0.0.1:0".parse().expect("bind"),
        store_dir: store,
        require_signed_ack: true,
        require_pow,
        ..GatewayConfig::default()
    })
    .expect("relay start")
}

fn identity(seed: u8) -> Identity {
    Identity::from_seed([seed; 32], "tgrin").expect("identity")
}

fn unique_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "flintmsg-test-{name}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn free_tcp_url() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    format!("http://127.0.0.1:{port}")
}

#[test]
fn direct_loopback_roundtrip_with_signed_ack() {
    let relay = start_relay(None, true);
    let alice = identity(1);
    let bob = identity(2);
    let alice_messenger = Messenger::new(GatewayTransport::new(&relay.url()).expect("alice"));
    let bob_messenger = Messenger::new(GatewayTransport::new(&relay.url()).expect("bob"));

    let envelope = alice_messenger
        .send(
            &alice,
            bob.address(),
            MessageKind::Text {
                text: "hello loopback".into(),
            },
            DEFAULT_TTL_SECS,
        )
        .expect("send");
    assert!(!envelope.id.is_empty());
    assert!(
        envelope.pow_bits >= flint_messenger::envelope::POW_MIN_BITS,
        "sealed envelope must carry enough PoW for the relay policy"
    );

    let batch = bob_messenger.fetch_open(&bob, 20).expect("fetch");
    assert_eq!(batch.opened.len(), 1, "one message expected");
    assert_eq!(
        batch.opened[0].sender.to_string(),
        alice.address().to_string()
    );
    match &batch.opened[0].kind {
        MessageKind::Text { text } => assert_eq!(text, "hello loopback"),
        other => panic!("unexpected kind: {other:?}"),
    }

    let acked = bob_messenger
        .ack_signed(&bob, &[batch.opened[0].id.clone()])
        .expect("signed ack");
    assert_eq!(acked, 1);
    let after_ack = bob_messenger.fetch_open(&bob, 20).expect("fetch again");
    assert!(after_ack.opened.is_empty(), "acked message must be gone");
}

#[test]
fn unsigned_ack_is_rejected_and_signed_ack_works() {
    let relay = start_relay(None, true);
    let alice = identity(3);
    let bob = identity(4);
    let transport = GatewayTransport::new(&relay.url()).expect("transport");
    let envelope = Envelope::seal_now(
        &alice,
        bob.address(),
        MessageKind::Text {
            text: "ack test".into(),
        },
        DEFAULT_TTL_SECS,
    )
    .expect("seal");
    transport.deliver(&envelope).expect("deliver");

    let ids = vec![envelope.id.clone()];
    let unsigned = transport.ack(bob.address(), &ids);
    assert!(unsigned.is_err(), "relay must reject unsigned ack");
    let signed = transport.ack_signed(&bob, &ids).expect("signed ack");
    assert_eq!(signed, 1);
    assert!(
        transport
            .fetch(bob.address(), envelope.created_at, 10)
            .expect("fetch")
            .is_empty(),
        "message must be removed after signed ack"
    );
}

#[test]
fn duplicate_delivery_is_stored_once() {
    let relay = start_relay(None, true);
    let alice = identity(5);
    let bob = identity(6);
    let transport = GatewayTransport::new(&relay.url()).expect("transport");
    let envelope = Envelope::seal_now(
        &alice,
        bob.address(),
        MessageKind::Text {
            text: "once".into(),
        },
        DEFAULT_TTL_SECS,
    )
    .expect("seal");
    transport.deliver(&envelope).expect("first deliver");
    transport.deliver(&envelope).expect("duplicate deliver");

    let bob_messenger = Messenger::new(GatewayTransport::new(&relay.url()).expect("bob"));
    let batch = bob_messenger.fetch_open(&bob, 20).expect("fetch");
    assert_eq!(batch.opened.len(), 1, "mailbox deduplicates by id");
}

#[test]
fn tampered_envelope_is_quarantined_not_opened() {
    // PoW is disabled here: flipping the ciphertext would also break the PoW
    // commitment and the relay would reject it before storage.
    let relay = start_relay(None, false);
    let alice = identity(7);
    let bob = identity(8);
    let transport = GatewayTransport::new(&relay.url()).expect("transport");
    let mut envelope = Envelope::seal_now(
        &alice,
        bob.address(),
        MessageKind::Text {
            text: "tamper".into(),
        },
        DEFAULT_TTL_SECS,
    )
    .expect("seal");
    envelope.ciphertext[0] ^= 0x01;
    transport.deliver(&envelope).expect("opaque delivery");

    let bob_messenger = Messenger::new(GatewayTransport::new(&relay.url()).expect("bob"));
    let batch = bob_messenger.fetch_open(&bob, 20).expect("fetch");
    assert!(batch.opened.is_empty(), "tampered envelope must not open");
    assert_eq!(
        batch.failed.len(),
        1,
        "tampered envelope must be quarantined"
    );
}

#[test]
fn relay_rejects_envelope_without_required_pow() {
    let relay = start_relay(None, true);
    let alice = identity(9);
    let bob = identity(10);
    let transport = GatewayTransport::new(&relay.url()).expect("transport");
    let mut envelope = Envelope::seal_now(
        &alice,
        bob.address(),
        MessageKind::Text {
            text: "no pow".into(),
        },
        DEFAULT_TTL_SECS,
    )
    .expect("seal");
    envelope.pow_bits = 0;
    let error = transport.deliver(&envelope);
    assert!(
        error.is_err(),
        "relay with require_pow must reject pow_bits=0"
    );
}

#[test]
fn relay_restart_keeps_stored_messages() {
    let dir = unique_dir("relay-store");
    let relay = start_relay(Some(dir.clone()), true);
    let alice = identity(11);
    let bob = identity(12);
    let alice_messenger = Messenger::new(GatewayTransport::new(&relay.url()).expect("alice"));
    alice_messenger
        .send(
            &alice,
            bob.address(),
            MessageKind::Text {
                text: "survives restart".into(),
            },
            DEFAULT_TTL_SECS,
        )
        .expect("send");
    drop(relay);

    let restarted = start_relay(Some(dir.clone()), true);
    let bob_messenger = Messenger::new(GatewayTransport::new(&restarted.url()).expect("bob"));
    let batch = bob_messenger.fetch_open(&bob, 20).expect("fetch");
    assert_eq!(
        batch.opened.len(),
        1,
        "file-backed mailbox survives restart"
    );
    match &batch.opened[0].kind {
        MessageKind::Text { text } => assert_eq!(text, "survives restart"),
        other => panic!("unexpected kind: {other:?}"),
    }
    drop(restarted);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn offline_delivery_is_an_error_not_a_silent_success() {
    let url = free_tcp_url();
    let alice = identity(13);
    let bob = identity(14);
    let messenger = Messenger::new(GatewayTransport::new(&url).expect("transport"));
    let error = messenger.send(
        &alice,
        bob.address(),
        MessageKind::Text {
            text: "nobody home".into(),
        },
        DEFAULT_TTL_SECS,
    );
    assert!(error.is_err(), "unreachable relay must fail delivery");
}

#[test]
fn onion_address_matches_tor_arti_derivation() {
    let seed = [42u8; 32];
    let identity = Identity::from_seed(seed, "tgrin").expect("identity");
    let onion = identity.address().onion();
    assert_eq!(onion.len(), 56);
    assert_eq!(onion, flint_tor_arti::onion_address(&seed));
    assert_eq!(
        identity.address().onion_url(),
        format!("http://{onion}.onion")
    );
}

#[test]
fn plaintext_slatepack_is_refused() {
    let alice = identity(15);
    let bob = identity(16);
    let error = Envelope::seal_now(
        &alice,
        bob.address(),
        MessageKind::Slatepack {
            armor: "BEGINSLATEPACK".into(),
            encrypted: false,
        },
        DEFAULT_TTL_SECS,
    );
    assert!(error.is_err(), "plaintext slatepack must not be sealed");
}
