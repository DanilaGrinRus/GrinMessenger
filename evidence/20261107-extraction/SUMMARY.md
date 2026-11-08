# Extraction summary (English)

Quick English summary of the extraction and its verification. The detailed
historical reports are kept in Russian: [REPORT.md](REPORT.md) (extraction
report) and [VERIFY.md](VERIFY.md) (independent verification), with raw logs in
[verify-raw/](verify-raw/).

## What was extracted

From the public [Flint](https://github.com/DanilaGrinRus/Flint) repository,
commit `20efa26a953f83a36ec0fe248e390d0031d01a95` (2026-10-30), the messenger
component was extracted into this standalone workspace:

| Crate | Purpose |
|---|---|
| `flint-messenger` | envelope v1, crypto, PoW, ACK, store/history/contacts, transports |
| `flint-tor-arti` | in-process Arti: onion v3 service from the ed25519 seed, HTTP over Tor |
| `flint-gateway` (`flint-relay`) | standalone store-and-forward relay without a wallet |
| `flint-messenger-cli` (`flintmsg`) | two-peer CLI: keygen/address/serve/send/fetch |
| `flint-messenger-ui` (`flintmsg-ui`) | local web UI: chat, contacts, envelope inspector, transcript export (added after the extraction; new code) |
| `flint-messenger-wallet` (`flintmsg-walletd`) | wallet integration layer: embedded `WalletService`, seed derivation, encrypted history/contacts/read markers, loopback JSON API sidecar (added 2026-11-08; new code) |

Wallet, Grin node, RPC, payment/Slatepack automation and the Slint UI were **not**
extracted. License: MIT OR Apache-2.0 (owner authorization; Apache-2.0 parts of
grin-wallet keep their own license). Full file mapping:
[PROVENANCE.md](../../PROVENANCE.md).

## Web UI (added 2026-11-08)

`flint-messenger-ui` serves one embedded HTML page plus a JSON API on loopback;
the Rust server is a blocking std `TcpListener` (no async runtime, no new
crates). Relay mode works without Tor; direct mode publishes an onion through
Arti. Screenshot of a live demo (relay on loopback, CLI peer ↔ browser):
[docs/assets/ui.png](../../docs/assets/ui.png). It has no authentication and
binds loopback only.

## Wallet integration (added 2026-11-08)

`flint-messenger-wallet` is the wallet-facing layer: a `WalletService` (identity
from a wallet-derived seed, encrypted history/contacts/read markers, poll/event
index with cursors) and `flintmsg-walletd`, a loopback sidecar JSON API for
non-Rust wallets. Wallets derive a dedicated messaging seed with
HKDF-SHA256 (`derive_messaging_seed`), so spending keys never enter the messenger.
Slatepack armor is transported opaquely (`MessageKind::Slatepack`, encrypted=true
enforced); the layer has no wallet RPC and does not create/finalize transactions.
The API requires a bearer token (except `/v1/health`), refuses non-loopback
binds, and the daemon takes the seed from a file/env/identity file (or generates
one explicitly). Contract: [docs/WALLET_INTEGRATION.md](../../docs/WALLET_INTEGRATION.md).
Evidence: [26-wallet-tests.log](26-wallet-tests.log), [27-wallet-demo.log](27-wallet-demo.log).

## Live Tor fix (2026-11-08)

The first live run exposed a real wire bug: hyper emitted absolute-form request
targets (`POST http://host/path`) while the gateway parsed only origin-form paths,
so direct delivery returned HTTP 404. Fixed in commit `d828211`:
`TorHttpClient` now sends origin-form plus an explicit `Host` header, and the
gateway also accepts absolute-form targets (regression test
`absolute_form_request_target_is_routed_correctly`). The GCP VM run after the
fix delivered a direct onion message end to end.

## Verification result

| Check | Result |
|---|---|
| Offline build (`cargo build --workspace --offline --locked`) | **PASS** (exit 0) |
| Automated tests (`cargo test --workspace --offline --locked`) | **PASS**: 142 passed, 0 failed, 2 ignored |
| `cargo fmt --check` | **PASS** |
| Ignored `relay_live` test (spawns the real `flint-relay` process) | **PASS**, 1 passed |
| Web-UI integration tests (real relay + real `flintmsg-ui` process) | **PASS**, 2 passed |
| Web-UI demo (relay loopback, CLI peer ↔ browser, screenshot) | **PASS**: delivery both ways, signed ACK, contact + transcript export |
| Wallet integration tests (two real `flintmsg-walletd` processes + real relay) | **PASS**, 3 integration + 11 unit tests: text and encrypted Slatepack delivery, contacts, unread/read, event cursors, bearer auth, validation |
| Wallet sidecar demo (API transcript, relay loopback) | **PASS**: capabilities/identity, contacts, text + Slatepack delivery, events, unread/read — [27-wallet-demo.log](27-wallet-demo.log) |
| **Live Tor** (cross-process direct onion on a GCP VM) | **PASS** (2026-11-08): Arti bootstrapped 100%, `flintmsg serve` published an onion, `flintmsg send` delivered a direct onion envelope, the receiver opened it and sent a signed ACK; log [25-live-tor-gcp.log](25-live-tor-gcp.log). The local build sandbox has no Tor egress. |
| Two-process loopback E2E (`flint-relay` + `flintmsg keygen/send/fetch`) | **PASS**: delivery, open, signed ACK, empty re-fetch, offline send fails |
| Independent verification (separate agent, own build dir) | **PASS**, no P0–P2 defects; 9 findings fixed and re-checked |
| Live Tor (onion service through Arti) | **FAILED / NOT VERIFIED / BLOCKED**: Arti bootstrap timed out after 300 s; the Tor network is unreachable in the build environment. Not reported as a pass. |

The 2 ignored tests are: `relay_live` (run explicitly and PASS) and the live
Nostr test (needs an external relay).

## Freeze manifest

The delivered source tree is pinned by
[source-sha256.sha256](source-sha256.sha256) (68 files: crates, Cargo manifests,
docs, site, screenshot, GitHub workflow, README, licenses, notices; `evidence/`
is intentionally excluded). Its SHA-256 is stored in
[source-sha256.sha256.hash](source-sha256.sha256.hash).
`upstream-sources.sha256` pins the 40 upstream files used by the extraction.

## Known limitations

- The local build sandbox has no Tor egress; live Tor was therefore verified on
  a GCP VM (two-process direct onion, see above). Direct-mode UI/CLI against a
  real onion is covered by that CLI run, not by a live UI run.
- The web UI has no authentication; it is loopback-only by design.
- Wallet integration transports wallet-provided Slatepack armor only: no wallet
  RPC, no automatic transaction processing, one identity per daemon, no iOS/FFI
  build in this version.
- No forward secrecy, no groups/attachments, no offline guarantee.
- Not audited; DEV/testnet tool; no mainnet readiness claim.
- Extraction evidence and reports are in Russian (historical artifacts).

## Publication

- Repository: <https://github.com/DanilaGrinRus/GrinMessenger> (public, `main`)
- Website: <https://danilagrinrus.github.io/GrinMessenger/> (GitHub Pages,
  deployed from `site/` by `.github/workflows/pages.yml`; run 42383391523 =
  success, HTTP 200 verified)
- The old Flint landing site (<https://danilagrinrus.github.io/Flint/>) was
  removed: the GitHub Pages API refused deactivation (HTTP 422), so the
  `gh-pages` branch was deleted, which disabled Pages automatically; the old URL
  now returns HTTP 404.
- The Flint repository itself is now an **archived pointer**: all sources,
  releases, tags and the old history on `master` were replaced by a single
  README linking to GrinMessenger; issues/projects/wiki are disabled. Full
  repository deletion requires the GitHub `delete_repo` token scope (not granted
  in this session); until then the pointer repository remains as requested.
- Documentation and the landing page are in English; the detailed extraction
  report and verifier report remain in Russian as historical artifacts.

## Reproduce

```powershell
Set-Location C:\DHW\GrinMessageModule
$env:TMP = "C:\DHW\GrinMessageModule\.tmp-build"; $env:TEMP = $env:TMP
$env:CARGO_TARGET_DIR = "C:\DHW\GrinMessageModule\target"
cargo build --workspace --offline --locked
cargo test  --workspace --offline --locked
cargo test -p flint-gateway --test relay_live --offline -- --ignored --test-threads=1
```
