// SPDX-License-Identifier: MIT OR Apache-2.0
//! Embedded wallet-service example (no sidecar, no HTTP token).
//!
//! A Rust wallet links `flint-messenger-wallet` and drives the service directly.
//! This example uses a **fixed demo seed**; a real wallet must derive a dedicated
//! messaging seed from its master seed (`derive_messaging_seed`) and keep it in
//! the wallet's own secret storage.
//!
//! Start a relay first, then run:
//!   cargo run -p flint-messenger-wallet --example embedded_wallet -- http://127.0.0.1:8080

use std::sync::Arc;
use std::time::Duration;

use flint_messenger_wallet::{NetworkMode, ServiceConfig, WalletService};
use zeroize::Zeroizing;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let relay = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://127.0.0.1:8080".to_string());
    let data_dir = std::env::temp_dir().join(format!(
        "flint-wallet-embedded-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    // Demo seed only. Production: derive with HMAC/HKDF from the wallet master
    // seed, e.g. `derive_messaging_seed(master, "grin-wallet/mainnet/account-0/messenger")`.
    let seed = Zeroizing::new([42u8; 32]);
    let service = Arc::new(WalletService::open(
        ServiceConfig {
            data_dir: data_dir.clone(),
            network: "tgrin".into(),
            mode: NetworkMode::Relay {
                url: relay.clone(),
                plain: true,
            },
            poll_secs: 1,
            fetch_limit: 100,
            ..ServiceConfig::default()
        },
        seed,
    )?);
    service.start_network();
    let status = service.wait_ready(Duration::from_secs(20));
    println!(
        "phase={} mode={} address={} unread={}",
        status.phase, status.mode, status.address, status.unread
    );
    println!("data_dir={}", data_dir.display());
    let events = service.events(0, Duration::from_secs(3));
    println!("events_seen={} next={}", events.events.len(), events.next);
    service.shutdown();
    std::fs::remove_dir_all(&data_dir).ok();
    Ok(())
}
