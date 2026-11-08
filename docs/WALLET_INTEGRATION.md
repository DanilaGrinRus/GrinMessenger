# Wallet integration

This document describes how a Grin wallet integrates GrinMessenger as its
messaging / Slatepack transport. It is written for two audiences:

- **Rust wallets** (e.g. `grin-wallet`) — link `flint-messenger-wallet` and use
  the typed API directly.
- **Non-Rust wallets** (e.g. Grin++/C++/Qt) — run the `flintmsg-walletd`
  sidecar and use its loopback JSON API.

The messenger is deliberately wallet-agnostic: it does **not** call wallet RPC,
does not create or finalize Slatepacks and does not touch spending keys. The
wallet decides when a payment needs a Slatepack; the messenger only carries the
already-encrypted armor between the two wallets.

---

## 1. What is provided

| Component | Purpose |
|---|---|
| `flint-messenger` | envelope v1 protocol: ed25519 signatures, X25519 + ChaCha20-Poly1305 encryption, PoW, signed ACKs |
| `flint-tor-arti` | in-process Arti: onion service and HTTP-over-Tor |
| `flint-gateway` (`flint-relay`) | optional store-and-forward relay; sees only opaque envelopes |
| `flint-messenger-wallet` | **integration layer**: wallet-facing service, seed derivation, encrypted local history/contacts/read markers |
| `flintmsg-walletd` | loopback sidecar JSON API for non-Rust wallets |
| `flintmsg`, `flintmsg-ui` | CLI and browser UI for demos/debugging |

Two transports are available per identity:

- **relay** — always-on store-and-forward: the recipient can be offline. Use a
  local `flint-relay`, your own server, or an onion relay. Relay mode with
  `plain = true` is loopback-only and intended for tests/dev.
- **direct** — the wallet publishes a Tor v3 onion derived from its ed25519
  address and delivers directly to the recipient's onion. No relay, but the
  recipient must be online. First Arti bootstrap takes 30–150 s.

---

## 2. Identity and seed handling

A Grin slatepack address is `bech32(ed25519 public key)`. The messenger uses the
same format (`grin1…` mainnet, `tgrin1…` testnet), so an address can be shown
next to the wallet's own slatepack address without a second directory.

The messenger identity is a **32-byte ed25519 seed**. There are three ways to
obtain it; choose explicitly in the wallet code:

### A. Dedicated messaging seed (recommended)

Derive a subkey from the wallet master seed with HKDF-SHA256 so the messenger can
never sign wallet transactions. The helper is implemented in
`flint_messenger_wallet::derive_messaging_seed`:

```text
messaging_seed = HKDF-SHA256(
    salt = "flint-messenger/wallet-integration/v1",
    ikm  = wallet_master_seed,
    info = "grin-wallet/mainnet/account-0/messenger",
    len  = 32)
```

Rules:

- keep `label` stable per wallet/account; changing it changes the address.
- do not store the derived seed; re-derive it at start from the seed the wallet
  already has.
- the messenger address is **different** from the wallet's slatepack address;
  show both in the UI ("payment address" vs "messenger address") or use the
  messenger address for chat/Slatepack transport.

### B. Reuse the wallet address key

If the wallet can export the 32-byte seed of its *address* key (not the spending
key), pass that seed. The messenger address then equals the wallet's slatepack
address. This is convenient for user recognition but reuses one ed25519 key for
two purposes (wallet address signatures and envelope signatures). Both uses are
domain-separated in this protocol, but a dedicated subkey (option A) is safer.

### C. Standalone/generated seed (tests, standalone chat)

`--generate --identity <file>` writes a JSON identity file (secret seed, `0600`
on Unix). This is for demos and for wallets that want a separate chat identity
not derived from the wallet seed. Never ship a generated seed as a default.

### Seed input to the daemon

Exactly one source is required:

- `--seed-file <file>` — one line: 64-char hex (optional `0x`) or base64 of 32 bytes.
- `--seed-env <VAR>` — read the seed from an environment variable.
- `--identity <file>` — JSON identity file; add `--generate` to create it.

The seed is never printed and is not written by the daemon (except when the
caller explicitly uses `--generate`/`--identity`). Use
`flintmsg-walletd identity --seed-file … --network tgrin` to print only the
derived address/onion before starting the service.

---

## 3. Sidecar quick start

Build:

```powershell
cd C:\DHW\GrinMessageModule
cargo build --workspace
```

Start a local relay (dev only) and the wallet daemon:

```powershell
target\debug\flint-relay.exe --listen 127.0.0.1:8080 --store relay-store
target\debug\flintmsg-walletd.exe `
  --data wallet-data\messenger `
  --seed-file wallet-data\messenger-seed.hex `
  --network tgrin `
  --relay http://127.0.0.1:8080 --plain `
  --listen 127.0.0.1:0
```

The daemon prints one JSON line and then serves:

```json
{"event":"ready","api":"http://127.0.0.1:50642",
 "token_file":"…\\wallet-data\\messenger\\api-token",
 "address":"tgrin1…","network":"tgrin","mode":"relay",
 "phase":"starting","api_version":1,"pid":1234,
 "token_env":"FLINTMSG_WALLETD_TOKEN"}
```

Read the token from `token_file` (never print it in logs) and call the API with
`Authorization: Bearer <token>`. `GET /v1/health` is the only unauthenticated
endpoint. The listener is forced to loopback: `--listen 0.0.0.0:…` is rejected.

> `phase` is `starting` at startup; poll `GET /v1/status` until `phase` is
> `ready` (relay: fast; direct: after Arti bootstrap, 30–150 s).

Direct mode (no relay):

```powershell
target\debug\flintmsg-walletd.exe `
  --data wallet-data\messenger --seed-file wallet-data\messenger-seed.hex `
  --network tgrin
```

---

## 4. HTTP API (`/v1`, bearer token)

All responses are JSON. Errors are `{"error":"…"}` with `400`, `401`, `404` or
`500`. Body limit: 1 MiB; request timeout 10 s (except long-poll `wait`).

| Method & path | Purpose |
|---|---|
| `GET /v1/health` | unauthenticated liveness + phase |
| `GET /v1/capabilities` | API/protocol versions, kinds, limits |
| `GET /v1/status` | phase, mode, address, onion, last error, unread counts |
| `GET /v1/identity` | address, onion, network, mode |
| `GET /v1/contacts` | list contacts |
| `POST /v1/contacts` | add/update: `{"address":"tgrin1…","name":"Bob"}` |
| `DELETE /v1/contacts/<address>` | remove a contact |
| `GET /v1/messages?after=<seq>&limit=<n>` | message history/live index |
| `GET /v1/events?since=<seq>&wait=<0..30>` | long-poll events |
| `GET /v1/unread` | `{"total":n,"peers":{address:n}}` |
| `POST /v1/read` | `{"peer":"tgrin1…"}` mark incoming as read |
| `POST /v1/send` | send text or Slatepack (below) |

### Send

```json
// text
{"to":"tgrin1…","kind":"text","text":"hello"}
// encrypted Slatepack armor (opaque to the messenger)
{"to":"tgrin1…","kind":"slatepack","armor":"BEGINSLATEPACK.…"}
// optional: "ttl_secs": 3600 (default 12 h, max 7 days)
```

Response:

```json
{"message":{"seq":7,"id":"…","peer":"tgrin1…","direction":"outgoing",
 "kind":"slatepack","armor":"…","encrypted":true,
 "created_at":1794100000,"expires_at":1794143200}}
```

Plaintext Slatepack is refused by protocol v1: the API always sends
`encrypted: true` and the receiving wallet must treat the armor as opaque.
Invalid kind, empty text/armor, oversized payload, unknown network address or a
non-ready transport return `400` / `401` with an explanatory error.

### Messages, events and cursors

`/v1/messages` returns records with a monotonic `seq`:

```json
{"epoch":"e3b0c442…","next":12,
 "messages":[{"seq":11,"id":"…","peer":"tgrin1…","direction":"incoming",
  "kind":"text","text":"hi","created_at":1794100000}]}
```

- `next` is the cursor to pass as `after` on the next call.
- `epoch` identifies the daemon's in-memory index. On daemon restart a new epoch
  is generated and `seq` values restart; when the epoch changes, the wallet must
  refetch from `after=0`.
- `/v1/events` returns live events since `since`: `message`, `rejected`
  (malformed/forged envelope), `status` (ready/stopped) and `error`. `wait`
  implements long-polling up to 30 s: one blocking call per wallet notification
  loop is enough.
- Both pages return at most 1000 messages; `messages` keeps up to 10 000 recent
  entries in memory but full history is persisted (see below). A wallet that
  attaches to an existing data dir should always start with `after=0`.

### Unread

`GET /v1/unread` counts incoming messages newer than the per-peer read marker.
`POST /v1/read` sets the marker to the newest incoming message of that peer.
Read markers survive restarts (encrypted file).

---

## 5. Slatepack exchange flow

1. Wallet A wants to pay wallet B. A's wallet builds an **encrypted Slatepack**
   (its own wallet API/library, exactly as for any Slatepack transport).
2. A posts the armor: `POST /v1/send {"to":"<B messenger address>","kind":"slatepack","armor":"…"}`.
3. B receives a `message` event / `incoming` record with `kind: "slatepack"` and
   the armor; B's wallet passes the armor to its own `slatepack` finalize API.
4. B's response Slatepack is sent back the same way; A finalizes.
5. The wallets complete the transaction through their normal wallet RPC. The
   messenger does not parse, modify or store Slatepack keys and does not have a
   wallet endpoint of its own.

A relay cannot read the envelope payload; direct mode avoids the relay entirely.
Slatepack armor is additionally encrypted by the wallet, so a malicious relay or
Tor exit sees two layers.

---

## 6. Rust embedded API

```rust
use std::sync::Arc;
use std::time::Duration;
use flint_messenger_wallet::{
    derive_messaging_seed, NetworkMode, ServiceConfig, WalletService,
};
use zeroize::Zeroizing;

// The wallet owns `master_seed`; derive a dedicated messaging seed.
let seed = derive_messaging_seed(&master_seed, "grin-wallet/mainnet/account-0/messenger");
let service = Arc::new(WalletService::open(
    ServiceConfig {
        data_dir: wallet_data_dir.join("messenger"),
        network: "grin".into(),
        mode: NetworkMode::Relay { url: "http://127.0.0.1:8080".into(), plain: true },
        ..ServiceConfig::default()
    },
    Zeroizing::new(seed),
)?);
service.start_network();
let status = service.wait_ready(Duration::from_secs(180));
assert_eq!(status.phase, "ready");

let message = service.send_text(&peer_address, "hello", None)?;
for event in service.events(0, Duration::from_secs(25)).events {
    // handle message/rejected/status/error
}
```

The typed service has the same methods as the HTTP API: `status`, `messages`,
`events`, `contacts`, `add_contact`, `remove_contact`, `unread`, `mark_read`,
`send_text`, `send_slatepack`, `shutdown`. For direct mode, network startup is
slow by design; keep a UI status indicator and never block the wallet UI thread
on `wait_ready`.

---

## 7. Data directory and backups

```text
<data>/identity.json     only with --generate/--identity: SECRET seed (0600 on Unix)
<data>/api-token         bearer token (0600 on Unix)
<data>/history/<peer>/…  encrypted message history (ChaCha20-Poly1305)
<data>/contacts/…        encrypted address book
<data>/state/read_state.enc  encrypted read markers
<data>/mailbox/          direct mode: local inbox (opaque envelopes)
<data>/arti-state/       Arti state/cache (no wallet secrets)
```

- History/contacts/read markers are encrypted with the storage key derived from
  the seed (domain-separated; see `storage_key`). They are useless without the
  seed, but still back them up consistently with the wallet.
- If the seed is derived from the wallet master seed (option A), the messenger
  data directory contains no spending keys and the wallet seed remains in the
  wallet process.
- Token file: delete it to rotate the token; the daemon creates a new one on the
  next start. With `--token-file` the wallet can provision its own token first
  (min 32 chars).

---

## 8. Security model and limitations

- Loopback only: the daemon refuses non-loopback binds. There is no TLS and no
  remote API. Protection against other local processes is the bearer token; the
  token file permissions should be checked by the supervising wallet.
- The seed is the identity. Do not log it, send it to telemetry, put it in
  command lines (`--seed-file`/`--seed-env` keep it out of the process table) or
  include it in crash reports.
- Envelope v1 has **no forward secrecy** and no deniability guarantees; metadata
  (peer addresses, timing) is visible to the relay/direct endpoint as usual.
- History/contacts are encrypted at rest but not forward-secret; deleting the
  messenger data directory removes the local history but not copies on the peer.
- The relay sees recipient addresses and ciphertext, can drop/delay messages, but
  cannot read or forge them. Signed ACKs let the recipient delete stored mail.
- The messenger is **not audited**; use on DEV/testnet first. Mainnet use is at
  the caller's risk.
- One identity per service/daemon. Multi-account wallets can run one service per
  account (separate data dirs) or derive different labels.
- iOS/mobile: there is no FFI/staticlib in this version and spawning a sidecar is
  not allowed on iOS; mobile support would need a Rust library build/FFI layer.

---

## 9. Wallet-specific notes

### grin-wallet (Rust)

Add `flint-messenger-wallet` as a dependency (or vendor the workspace), derive
the messaging seed (option A) and embed `WalletService`. No daemon, no extra
ports, no token needed. Show the messenger address next to the slatepack address;
use the `events` API from the wallet's notification loop. If grin-wallet is used
as a CLI only, the sidecar works too.

### Grin++ / C++ / Qt

Spawn `flintmsg-walletd` as a child process, read the first stdout line, read
the token file, and talk HTTP to the printed `api` URL with the wallet's HTTP
client (e.g. `QNetworkAccessManager`). Ship the daemon per platform; restart it
on crash with the same `--data`/`--seed-file`. Use `status.phase` in the wallet
status bar: direct mode takes 30–150 s on first start. Prefer relay mode for an
always-on wallet unless direct-only operation is required.

### Relay operators / RPC

The module has no wallet RPC proxy (deliberately removed during extraction).
A wallet that wants to use a public relay configures its URL; the relay is
wallet-agnostic and stores only opaque envelopes with TTL/quotas/PoW.

---

## 10. Versioning, compatibility and limits

- API: `/v1`; `GET /v1/capabilities` reports API/protocol versions and limits.
- Envelope protocol v1; no silent fallback between transports.
- Limits: text 8 KiB, Slatepack armor 32 KiB, TTL 1 s–7 days, message page 1000,
  event wait ≤30 s, contacts ≤500.
- Direct-mode startup needs the Tor network; the original offline build sandbox
  could not reach it, so live Tor was verified on a GCP VM
  (`evidence/20261107-extraction/25-live-tor-gcp.log`).

## 11. Troubleshooting

| Symptom | Check |
|---|---|
| `401` on every call | read token file, send `Authorization: Bearer …`; tokens are not printed by the daemon |
| `phase: "error"` / `last_error` | relay URL reachable? Tor network available for onion/direct? |
| direct mode stays `starting` | Arti bootstrap can take 30–150 s; check the network |
| messages arrive only after restart | `seq` cursors reset with a new `epoch`; refetch with `after=0` |
| contact add fails with network message | address HRP (`grin`/`tgrin`) must match `--network` |
| token lost | stop daemon, delete `api-token`, restart (new token) |
| two wallets on one machine | use different `--data` and `--listen 127.0.0.1:0` (each gets a free port) |

## 12. Tests

```powershell
cargo test -p flint-messenger-wallet
```

Covers: seed derivation/domain separation, identity file roundtrip, encrypted read
markers, capabilities/bind policy, two real daemons exchanging text and encrypted
Slatepack through a real relay, unread/read markers, event cursors, bearer-token
auth and request validation. See
[`crates/flint-messenger-wallet/tests/wallet_api.rs`](../crates/flint-messenger-wallet/tests/wallet_api.rs)
and evidence logs `26-wallet-tests.log` / `27-wallet-demo.log`.
