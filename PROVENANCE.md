# Provenance — GrinMessenger

Published at <https://github.com/DanilaGrinRus/GrinMessenger> (2026-11-07; the
repository was created as `GrinMessageModule` and renamed to `GrinMessenger` the
same day). The crates and binaries keep their historical `flint-*` /
`flintmsg` / `flint-relay` names so the extraction provenance stays unambiguous.

## Source

| | |
|---|---|
| Repository | <https://github.com/DanilaGrinRus/Flint> |
| Branch | `master` |
| Commit | `20efa26a953f83a36ec0fe248e390d0031d01a95` (2026-10-30) |
| Extraction date | 2026-11-07 |
| Source checkout | read-only clone at `C:\DHW\tmp\flint-upstream` |
| Source hash manifest | [evidence/20261107-extraction/upstream-sources.sha256](evidence/20261107-extraction/upstream-sources.sha256) |

The source repository was the canonical upstream for this extraction. No files
were recovered from deleted local copies, user wallets, installed application
data, or the mobile repository.

## License transition

The upstream repository is licensed **GPL-3.0-only**. The owner of the copyright
on the Flint code authorized the extraction and relicensing of the
extracted/adapted portions under **MIT OR Apache-2.0** (2026-11-07).
Third-party portions keep their own licenses:

| Portion | License |
|---|---|
| Flint-authored code (`crates/messenger`, `gateway`, relay binary, docs, site) | MIT OR Apache-2.0 |
| New GrinMessenger code (`crates/flint-messenger-ui`, `flint-messenger-cli`, screenshot, landing-page content) | MIT OR Apache-2.0 |
| `flint-tor-arti` Tor integration, adapted from grin-wallet `impls/src/tor/*` | Apache-2.0 (derived from Apache-2.0 code; redistributed under Apache-2.0 and compatible with the project's MIT OR Apache-2.0 licensing) |
| External crates (arti 0.43, tor-* 0.43, hyper, ring, …) | their own licenses, see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and `Cargo.lock` |

`SPDX-License-Identifier: MIT OR Apache-2.0` headers are added to the extracted
files. grin-wallet-derived files carry an additional note about their Apache-2.0
origin.

## File mapping

| Upstream (Flint `20efa26a`) | This project | Status |
|---|---|---|
| `crates/messenger/src/*.rs`, `src/nostr/*.rs`, `Cargo.toml`, `examples/rewrap-key.rs` | `crates/flint-messenger/` | copied, SPDX headers added, transport extended with `HttpConnector` |
| `crates/messenger/tests/relay_live.rs` | `crates/flint-gateway/tests/relay_live.rs` | adapted to the standalone `flint-relay` binary |
| `crates/core/src/gateway.rs` | `crates/flint-gateway/src/gateway.rs` | adapted: wallet proxy (`/v2/foreign`) and `reqwest` removed; envelope address validation via `flint_messenger::Address`; all messenger API, quotas, TTL, PoW and signed-ACK rules preserved |
| `crates/core/src/bin/flint-relay.rs` | `crates/flint-gateway/src/bin/flint-relay.rs` | adapted: wallet references removed |
| `crates/core/src/error.rs` | `crates/flint-gateway/src/error.rs` | reduced to the variants the relay uses |
| `crates/core/src/onion.rs` (Arti onion service wrapper) | `crates/flint-tor-arti/src/onion.rs` | adapted |
| `third_party/grin-wallet/impls/src/tor/arti.rs` | `crates/flint-tor-arti/src/arti.rs` | adapted to stand alone: grin-wallet/grin types replaced with local `TorConfig`, `[u8; 32]` seeds and `TorError`; same arti 0.43 bootstrap, keystore and onion-service mechanics |
| `third_party/grin-wallet/impls/src/tor/config.rs` (`exp_sec_key_bytes`) | `crates/flint-tor-arti/src/keys.rs` | helper copied (Apache-2.0) |
| `crates/core/tests/onion_service.rs` | `crates/flint-tor-arti/tests/live_onion.rs` | adapted, `#[ignore]` (needs the Tor network) |
| `docs/contracts/MESSENGER.md` | `docs/PROTOCOL.md` | adapted for the standalone project (wallet/slatepack auto-processing removed, translated to English) |
| `docs/ARCHITECTURE.md`, `README.md` | `docs/ARCHITECTURE.md`, `README.md` | rewritten for the standalone project in English |
| `site/en/index.html`, `site/styles.css`, `site/assets/logo.svg` | `site/` | landing page/design adapted for GrinMessenger (new English content and logo, same palette/layout); source assets in the same repository |
| — (new code, 2026-11-08) | `crates/flint-messenger-ui/`, `docs/assets/ui.png`, `site/assets/ui.png` | local web UI: blocking HTTP server + embedded page, integration tests and a live demo screenshot; not copied from Flint |
| `crates/core/address.rs`, `crates/core/tor_listener.rs`, `crates/core/tests/direct_onion.rs`, `crates/core/tests/tor_listener_gateway.rs` | — | reference only; external `tor.exe`/SOCKS path is replaced by in-process Arti |
| `crates/core`, `crates/runtime`, `crates/slint-ui`, `crates/janus`, `crates/transfer-contract`, `third_party/grin-wallet`, `crates/grin-wallet-rpc` | — | **not extracted** (wallet, node, UI, payment and build infrastructure) |

## Modifications introduced by the extraction

1. Workspace split: a new Cargo workspace with four crates and no wallet, node,
   Slint UI or grin dependencies.
2. `flint-messenger`: `HttpConnector`/`TcpConnector` abstraction added so the
   same gateway protocol can run over in-process Arti; legacy constructors and
   behaviour preserved.
3. `flint-gateway`: wallet foreign-API proxy removed; the relay is a pure
   store-and-forward service for opaque envelopes. No wallet endpoint is
   reachable through the relay.
4. `flint-tor-arti`: standalone in-process Arti layer; the external `tor.exe`
   launch path, `torrc` bridges file handling and `TOR expert bundle` lookup are
   not part of this crate. Bridge configuration is passed through `TorConfig` and
   supported by the arti client.
5. Documentation adapted; non-extracted wallet/Slatepack integration statements
   removed instead of being carried over as unverified claims.
6. English translation of the project documentation and the landing page
   (2026-11-07); detailed extraction reports under `evidence/` are kept in
   Russian as historical artifacts.
7. Repository and project are published as **GrinMessenger**; the module's
   historical crate/binary names are unchanged.
8. Local web UI added (2026-11-08): `crates/flint-messenger-ui` (Rust std HTTP
   server + one embedded HTML page), integration tests, live demo screenshot and
   a landing-page section. It adds no new external crates.

## Reproducing the provenance check

```powershell
# from the extraction workspace
Get-FileHash -Algorithm SHA256 <upstream-file>   # compare with the manifest
```

The upstream checkout is kept outside the deliverable and is not required to
build the project. The `source-sha256.sha256` freeze manifest in
`evidence/20261107-extraction/` pins the delivered source tree.
