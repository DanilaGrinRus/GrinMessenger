# GrinMessenger

Standalone Grin messenger module, extracted from the
[Flint](https://github.com/DanilaGrinRus/Flint) project: signed and encrypted
envelope messages, direct delivery to a Tor v3 onion through **in-process Arti**,
and an optional store-and-forward relay. No wallet, no Grin node, no RPC and no
GUI are required.

Repository: <https://github.com/DanilaGrinRus/GrinMessenger> ·
Website: <https://danilagrinrus.github.io/GrinMessenger/>

The crates and binaries keep their historical `flint-*` / `flintmsg` /
`flint-relay` names as provenance of the extraction from the Flint messenger;
this is not the Flint wallet.

| | |
|---|---|
| Repository | <https://github.com/DanilaGrinRus/GrinMessenger> |
| Website | <https://danilagrinrus.github.io/GrinMessenger/> |
| License | MIT OR Apache-2.0 (see `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`) |
| Upstream | Flint `20efa26a953f83a36ec0fe248e390d0031d01a95` (2026-10-30) |
| Provenance | [PROVENANCE.md](PROVENANCE.md), [evidence/20261107-extraction](evidence/20261107-extraction/) |
| Protocol | [docs/PROTOCOL.md](docs/PROTOCOL.md) |
| Architecture | [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| Third-party components | [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) |

Status (2026-11-07): **extraction and adaptation are complete; offline build is
PASS, 125 automated tests are PASS (0 failed, 3 ignored), the loopback E2E and
real CLI processes are PASS. Live Tor is FAILED / NOT VERIFIED: in the build
environment Arti did not finish bootstrap within 300 s because the Tor network is
unreachable. This is not reported as a pass.** See
[evidence/20261107-extraction/SUMMARY.md](evidence/20261107-extraction/SUMMARY.md)
and [REPORT.md](evidence/20261107-extraction/REPORT.md) (the detailed extraction
report and verifier logs are kept in Russian as historical evidence).
This is a DEV/testnet tool: not audited, not ready for mainnet.

## Crates

| Crate | Binary | What it does |
|---|---|---|
| `flint-messenger` | — | envelope v1, crypto, PoW, ACK, store/history/contacts, transports (`GatewayTransport`, `DirectOnionTransport`, Nostr) |
| `flint-tor-arti` | — | in-process Tor: onion v3 from the ed25519 seed, onion service, HTTP client through Arti |
| `flint-gateway` | `flint-relay` | loopback gateway / standalone store-and-forward relay without a wallet |
| `flint-messenger-cli` | `flintmsg` | two-peer CLI: keygen / address / serve / send / fetch |

## Build

Rust for MSVC is required (edition 2021, `rust-version = "1.92"`; verified with
1.99).

```powershell
cargo build --workspace
cargo test --workspace
```

The repository is developed inside `C:\DHW`, where the inherited
`C:\DHW\.cargo\config.toml` replaces crates.io with the offline vendor at
`C:\DHW\vendor`, so the build works without network access. On another machine,
plain `cargo build` will fetch the pinned dependencies from `Cargo.lock`; for a
fully offline build, copy the vendor directory and a `.cargo/config.toml` next to
the project.

`profile.dev` sets `debug = 0` (MSVC chokes on long paths in the dependency
graph). Release build: `cargo build --release --workspace`.

## Quick start: direct mode (Tor/Arti)

A Tor network connection is required (Arti bootstrap; the first run can take
30–150 s). Both peers must be online: direct delivery stores nothing outside the
recipient. For a run without Tor, see relay mode below.

```powershell
# 1) identities (seed files; do NOT publish them)
cargo run -p flint-messenger-cli --bin flintmsg -- keygen --out alice.json
cargo run -p flint-messenger-cli --bin flintmsg -- keygen --out bob.json
cargo run -p flint-messenger-cli --bin flintmsg -- address --identity bob.json
# { "address": "tgrin1…", "onion": "….onion" }

# 2) Bob publishes the onion + loopback gateway and prints incoming JSON lines
cargo run -p flint-messenger-cli --bin flintmsg -- serve --identity bob.json --data bob-data

# 3) Alice sends to Bob's address (the onion is derived from the address)
cargo run -p flint-messenger-cli --bin flintmsg -- send --identity alice.json `
  --to tgrin1… --text "hello over arti"
```

`serve` prints `{"event":"ready",…}` and then one `{"event":"message",…}` line
per message; after printing, the message is acknowledged with a signed ACK. The
first Arti bootstrap of the client/service can take up to 150 s (the timeout is
configurable in `flint-tor-arti`).

## Relay mode (store-and-forward)

```powershell
# relay (loopback for testing; publish it through an onion in production)
cargo run -p flint-gateway --bin flint-relay -- --listen 127.0.0.1:8080 --store relay-store

# send and receive through the relay; --plain = TCP without Tor (loopback/clearnet only)
cargo run -p flint-messenger-cli --bin flintmsg -- send --identity alice.json `
  --to tgrin1… --text "hi via relay" --relay http://127.0.0.1:8080 --plain
cargo run -p flint-messenger-cli --bin flintmsg -- fetch --identity bob.json `
  --source http://127.0.0.1:8080 --plain
```

Without `--plain`, the CLI goes through Arti and can use a `.onion` relay.
`flint-relay` enables `require_pow` and `require_signed_ack`; messages are opaque
envelopes, so the relay cannot read the plaintext.

## Using it as a library

```rust
use flint_messenger::{Address, Identity, MessageKind, Messenger, DEFAULT_TTL_SECS};
use flint_messenger::transport::GatewayTransport;

let identity = Identity::from_seed(seed, "tgrin")?;
let recipient: Address = "tgrin1…".parse()?;
let messenger = Messenger::new(GatewayTransport::new("http://127.0.0.1:8080")?);
let envelope = messenger.send(
    &identity,
    &recipient,
    MessageKind::Text { text: "hello".into() },
    DEFAULT_TTL_SECS,
)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

To deliver through Arti, implement `flint_messenger::transport::HttpConnector`
on top of `flint_tor_arti::TorHttpClient` (this is what `flintmsg` does) and pass
it to `GatewayTransport::with_connector` / `DirectOnionTransport::with_connector`.
For an external Tor with SOCKS5, `GatewayTransport::with_socks` and
`DirectOnionTransport::new` are preserved.

## Checks

```powershell
cargo test --workspace                                      # unit + integration, no live Tor
cargo test -p flint-tor-arti --test live_onion -- --ignored  # needs the Tor network
cargo test -p flint-gateway --test relay_live -- --ignored    # spawns flint-relay
```

Run results (commands, PASS/FAIL, live Tor NOT VERIFIED when the network is
unavailable) are in [evidence/20261107-extraction/](evidence/20261107-extraction/).
Summary: `cargo build --workspace --offline --locked` — PASS;
`cargo test --workspace --offline --locked` — 125 passed / 0 failed / 3 ignored
(`relay_live`, live Nostr, `live_onion`); `cargo fmt --check` — PASS.

## Not included

- Wallet, Grin node, owner/foreign API, Slatepack transactions and their
  automatic processing (S1/S2).
- The Flint desktop Slint UI and the iOS client.
- Groups, attachments, ratchet/forward secrecy, guaranteed offline delivery,
  public abuse control and a custom P2P protocol.
- The Nostr transport exists in the library as a separate delivery option, but
  the v1 CLI does not enable it.

## Limitations and honest claims

- Protocol v1 has no forward secrecy and no post-compromise protection.
- Relay/direct delivery exposes metadata (recipient address, size, time); the
  relay cannot read the payload, but it can drop or duplicate messages.
- Tor hides the network path but is not a replacement for end-to-end
  encryption — the envelope provides that.
- The project does not claim anonymity, absence of bugs or mainnet readiness.
- The CLI keeps no persistent history/contacts: it is a demonstration tool; the
  library `History`, `ContactBook` and `FileMailbox` are available for embedding.
