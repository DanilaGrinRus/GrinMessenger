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

## Envelope v2 (2026-11-08)

Envelope **v2** moves the sender's ed25519 authentication inside the encrypted
payload (domain-separated sender transcript over sender/recipient/id/time/ttl/
kind), so a relay or wire observer cannot test a known candidate sender against
the public envelope. The outer `signature` field is `[0;64]` for v2 and unused;
`Envelope::open` still accepts legacy v1 envelopes (which retain the old
candidate-check leak), and `seal_legacy_v1` exists for interoperability tests.
New clients send v2; v1-only clients cannot open v2 messages (documented in
PROTOCOL §10). Evidence: [32-envelope-v2-tests.log](32-envelope-v2-tests.log),
[33-envelope-v2-demo.log](33-envelope-v2-demo.log).

## Release / CI (2026-11-08)

GitHub Actions `ci` runs a Windows+Linux build/test matrix, `cargo fmt --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, the real `relay_live`
process test, and `cargo audit` with documented advisory exceptions. The `release`
workflow publishes both platform bundles, per-platform `SHA256SUMS-*.txt`,
`Cargo.lock.release` and a CycloneDX SBOM. First release:
<https://github.com/DanilaGrinRus/GrinMessenger/releases/tag/v0.1.0>. See
[docs/RELEASE.md](../../docs/RELEASE.md).

## Security hardening (2026-11-08)

Audit findings and residual risks are tracked in
[docs/SECURITY.md](../../docs/SECURITY.md). Fixed in this release:

- **UI CSRF / DNS rebinding**: per-start session token printed only in the URL
  fragment, required in `X-Flint-UI-Token`; exact loopback `Host` allowlist;
  same-origin `Origin`; JSON POST only; loopback bind enforced.
- **Relay mailbox metadata**: `GET /flint/v1/msg` now requires a signed
  address+timestamp fetch proof; `flint-relay` enables it by default, clients
  sign automatically (`Transport::fetch_signed`).
- **UUID case handling**: ids are canonicalized (lowercase hyphenated) for
  store/ACK/list; mixed-case requests cannot create or address a second entry.
- **Direct-mode inbound DoS**: the shared loopback post-rate limit (all Tor
  senders appear as 127.0.0.1) is disabled for direct gateways; larger queue
  (500/recipient, 5000 total) + PoW + poll/ACK drain replace it.
- **Secret files**: wallet tokens/identity are written owner-only on Unix
  (created `0600` before the secret) and ACL-hardened on Windows; CLI identity
  files are `0600`; UI data files are owner-only.
- **Wallet console**: loopback `Host`/`Origin`/JSON guard and server-side token
  injection (the browser never sees the bearer token).

Residual limitations: legacy v1 envelopes (sender candidate check), address-key
reuse (online-time linkability), a sustained direct-mode flood can still delay
delivery (no per-sender authenticated quota), and the v1-only client
compatibility gap.

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
| Automated tests (`cargo test --workspace --offline --locked`) | **PASS**: 156 passed, 0 failed, 2 ignored |
| `cargo fmt --check` | **PASS** |
| `cargo clippy --workspace --all-targets -- -D warnings` | **PASS** |
| CI: Windows + Linux build/test/clippy matrix, dependency audit | **PASS** (GitHub Actions `ci` workflow; audit exceptions documented in `docs/SECURITY.md`) |
| Ignored `relay_live` test (spawns the real hardened `flint-relay` process) | **PASS**, 1 passed |
| Web-UI integration tests (real relay + real `flintmsg-ui` process, browser guard) | **PASS**, 4 passed |
| Web-UI demo (relay loopback, CLI peer ↔ browser, screenshot) | **PASS**: delivery both ways, signed ACK, contact + transcript export |
| Wallet integration tests (two real `flintmsg-walletd` processes + real relay) | **PASS**, 3 integration + 12 unit tests: text and encrypted Slatepack delivery, contacts, unread/read, event cursors, bearer auth, validation |
| Wallet web console tests (page/proxy/token non-exposure/DNS-rebinding guard) | **PASS**, 3 integration + 2 unit tests; [28-wallet-ui-demo.log](28-wallet-ui-demo.log) |
| Security hardening tests (signed fetch proof, canonical UUIDs, direct-mode burst, private files, UI/console guard) | **PASS**, covered by the workspace suite and the real relay process; [29-security-tests.log](29-security-tests.log), [30-security-demo.log](30-security-demo.log) |
| Envelope v2 (sender proof inside ciphertext, v1 compatibility) | **PASS**: v2 roundtrip, zero outer signature, candidate-sender check impossible for v2, forged sender proof rejected, legacy v1 open/tamper tests; [32-envelope-v2-tests.log](32-envelope-v2-tests.log), [33-envelope-v2-demo.log](33-envelope-v2-demo.log) |
| Release pipeline: GitHub Release `v0.1.0` with Linux/Windows binaries, per-platform SHA-256, `Cargo.lock.release`, CycloneDX SBOM | **PASS** ([release](https://github.com/DanilaGrinRus/GrinMessenger/releases/tag/v0.1.0)) |
| **Live Tor** (cross-process direct onion on a GCP VM) | **PASS** (2026-11-08): Arti bootstrapped 100%, `flintmsg serve` published an onion, `flintmsg send` delivered a direct onion envelope, the receiver opened it and sent a signed ACK; log [25-live-tor-gcp.log](25-live-tor-gcp.log). The local build sandbox has no Tor egress. |
| Two-process loopback E2E (`flint-relay` + `flintmsg keygen/send/fetch`) | **PASS**: delivery, open, signed ACK, empty re-fetch, offline send fails |
| Independent verification (separate agent, own build dir) | **PASS**, no P0–P2 defects; 9 findings fixed and re-checked |
| Live Tor (onion service through Arti) | **FAILED / NOT VERIFIED / BLOCKED**: Arti bootstrap timed out after 300 s; the Tor network is unreachable in the build environment. Not reported as a pass. |

The 2 ignored tests are: `relay_live` (run explicitly and PASS) and the live
Nostr test (needs an external relay).

## Freeze manifest

The delivered source tree is pinned by
[source-sha256.sha256](source-sha256.sha256) (74 files: crates, Cargo manifests,
docs, site, screenshot, GitHub workflow, README, licenses, notices; `evidence/`
is intentionally excluded). Its SHA-256 is stored in
[source-sha256.sha256.hash](source-sha256.sha256.hash).
`upstream-sources.sha256` pins the 40 upstream files used by the extraction.

## Known limitations

- The local build sandbox has no Tor egress; live Tor was therefore verified on
  a GCP VM (two-process direct onion, see above). Direct-mode UI/CLI against a
  real onion is covered by that CLI run, not by a live UI run.
- The web UI uses a per-start session token + Host/Origin guard; it is still a
  single-user local tool and must not be exposed.
- Wallet integration transports wallet-provided Slatepack armor only: no wallet
  RPC, no automatic transaction processing, one identity per daemon, no iOS/FFI
  build in this version.
- Envelope v2 hides the sender from a relay/wire observer; legacy v1 envelopes
  still expose the candidate-check leak and v1-only clients cannot open v2
  messages.
- Address-key reuse links the messenger onion to the payment address; derive a
  dedicated messaging seed.
- Direct-mode delivery is finite-capacity: a sustained flood can still delay
  messages (no sender-authenticated quota in v1).
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
