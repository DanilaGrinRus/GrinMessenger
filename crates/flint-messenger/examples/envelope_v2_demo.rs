// SPDX-License-Identifier: MIT OR Apache-2.0
//! Envelope v2 sender-anonymity demonstration.
//!
//! Run with:
//!   cargo run -p flint-messenger --example envelope_v2_demo
//!
//! v2 keeps the sender proof inside the ciphertext, so the wire JSON has no
//! verifiable sender signature and no sender address; the recipient verifies it
//! after decryption. v1 is shown for compatibility: it carries a public sender
//! signature and therefore allows a relay to test a candidate sender.

use flint_messenger::{Envelope, Identity, MessageKind, DEFAULT_TTL_SECS};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let alice = Identity::generate("tgrin")?;
    let bob = Identity::generate("tgrin")?;
    let now = flint_messenger::envelope::unix_now();

    let v2 = Envelope::seal(
        &alice,
        bob.address(),
        MessageKind::Text {
            text: "v2 sender proof is inside the ciphertext".into(),
        },
        DEFAULT_TTL_SECS,
        now,
    )?;
    let wire = serde_json::to_value(&v2)?;
    let opened = v2.open(&bob, now + 1)?;
    println!("=== envelope v2 ===");
    println!("version: {}", v2.version);
    println!(
        "outer signature (must be zero/empty proof): {}",
        wire["signature"]
    );
    println!(
        "sender address in wire: {}",
        wire.to_string().contains(&alice.address().to_string())
    );
    println!(
        "recipient in wire: {}",
        wire.to_string().contains(&bob.address().to_string())
    );
    println!(
        "recipient opened sender: {}",
        opened.sender == *alice.address()
    );
    println!("relay can test a known sender candidate: no (no public sender proof)");

    let v1 = Envelope::seal_legacy_v1(
        &alice,
        bob.address(),
        MessageKind::Text {
            text: "legacy v1".into(),
        },
        DEFAULT_TTL_SECS,
        now,
    )?;
    let v1_wire = serde_json::to_value(&v1)?;
    let legacy_opened = v1.open(&bob, now + 1)?;
    println!("\n=== legacy envelope v1 ===");
    println!("version: {}", v1.version);
    println!("outer signature non-zero: {}", v1.signature != [0u8; 64]);
    println!("outer signature field: {}", v1_wire["signature"]);
    println!(
        "v1 still opens: {}",
        legacy_opened.sender == *alice.address()
    );
    println!("v1 caveat: relay/observer can test a candidate sender against the public signature");
    Ok(())
}
