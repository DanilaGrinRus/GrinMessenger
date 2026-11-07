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

## Verification result

| Check | Result |
|---|---|
| Offline build (`cargo build --workspace --offline --locked`) | **PASS** (exit 0) |
| Automated tests (`cargo test --workspace --offline --locked`) | **PASS**: 127 passed, 0 failed, 3 ignored |
| `cargo fmt --check` | **PASS** |
| Ignored `relay_live` test (spawns the real `flint-relay` process) | **PASS**, 1 passed |
| Web-UI integration tests (real relay + real `flintmsg-ui` process) | **PASS**, 2 passed |
| Web-UI demo (relay loopback, CLI peer ↔ browser, screenshot) | **PASS**: delivery both ways, signed ACK, contact + transcript export |
| Two-process loopback E2E (`flint-relay` + `flintmsg keygen/send/fetch`) | **PASS**: delivery, open, signed ACK, empty re-fetch, offline send fails |
| Independent verification (separate agent, own build dir) | **PASS**, no P0–P2 defects; 9 findings fixed and re-checked |
| Live Tor (onion service through Arti) | **FAILED / NOT VERIFIED / BLOCKED**: Arti bootstrap timed out after 300 s; the Tor network is unreachable in the build environment. Not reported as a pass. |

The 3 ignored tests are: `relay_live` (run explicitly and PASS), the live Nostr
test (needs an external relay) and `live_onion` (needs the Tor network).

## Freeze manifest

The delivered source tree is pinned by
[source-sha256.sha256](source-sha256.sha256) (60 files: crates, Cargo manifests,
docs, site, screenshot, GitHub workflow, README, licenses, notices; `evidence/`
is intentionally excluded). Its SHA-256 is stored in
[source-sha256.sha256.hash](source-sha256.sha256.hash).
`upstream-sources.sha256` pins the 40 upstream files used by the extraction.

## Known limitations

- Live Tor/direct-onion CLI/UI is unverified in this environment.
- The web UI has no authentication; it is loopback-only by design.
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
