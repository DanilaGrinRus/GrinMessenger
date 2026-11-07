// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! Live Tor/Arti roundtrip: publish an onion service in front of a local stub
//! server, then reach it through a second Arti HTTP request.
//!
//! Requires the Tor network. Run explicitly:
//!
//! ```text
//! cargo test -p flint-tor-arti --test live_onion -- --ignored --nocapture
//! ```
//!
//! A pass here proves the in-process Arti path only for this machine/network;
//! without it the result is NOT VERIFIED, never a pass.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{SystemTime, UNIX_EPOCH};

use flint_tor_arti::{onion_address, OnionService, TorConfig, TorHttpClient};

/// Print Arti's bootstrap progress to stderr so a network-blocked run leaves a
/// diagnosable trace instead of a bare timeout.
fn install_test_logger() {
    struct StderrLogger;
    impl log::Log for StderrLogger {
        fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            eprintln!("[arti {:5}] {}", record.level(), record.args());
        }
        fn flush(&self) {}
    }
    static LOGGER: StderrLogger = StderrLogger;
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Info);
}

/// Minimal blocking HTTP/1.1 stub: answers every request with a fixed body.
fn spawn_stub(body: &'static str) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub");
    let addr = listener.local_addr().expect("stub addr").to_string();
    let handle = std::thread::spawn(move || {
        for stream in listener.incoming().take(4) {
            let Ok(mut stream) = stream else { break };
            let mut buffer = [0u8; 8192];
            let _ = stream.read(&mut buffer);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    (addr, handle)
}

#[test]
#[ignore = "needs the live Tor network"]
fn live_onion_service_roundtrip() {
    install_test_logger();
    let (stub_addr, stub) = spawn_stub("onion-ok");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let state_dir = std::env::temp_dir().join(format!(
        "flint-tor-arti-live-{}-{nanos}",
        std::process::id()
    ));
    let mut config = TorConfig::new(&state_dir);
    config.bootstrap_timeout = std::time::Duration::from_secs(300);
    config.request_timeout = std::time::Duration::from_secs(120);
    let seed = [7u8; 32];
    let expected = onion_address(&seed);

    let service = OnionService::start(&seed, &stub_addr, &config)
        .unwrap_or_else(|e| panic!("onion service start: {e}"));
    assert_eq!(service.onion(), expected);
    println!("onion: {}", service.url());

    // Give the service a moment to publish its descriptor.
    std::thread::sleep(std::time::Duration::from_secs(5));

    let client = TorHttpClient::bootstrap(&config).unwrap_or_else(|e| panic!("client: {e}"));
    let url = format!("{}/", service.url());
    let (status, body) = client
        .request("GET", &url, b"")
        .unwrap_or_else(|e| panic!("request {url}: {e}"));
    assert_eq!(status, 200, "status {status}");
    assert_eq!(String::from_utf8_lossy(&body), "onion-ok");

    service.stop();
    stub.join().ok();
    std::fs::remove_dir_all(&state_dir).ok();
}
