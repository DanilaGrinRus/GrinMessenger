# GrinMessenger architecture

## Crates

```text
flintmsg (CLI)  ─────┐
flintmsg-ui (web) ───┤
flintmsg-walletd ────┴──► flint-messenger   envelope, crypto, PoW, ACK, store, transport
                        ├── flint-tor-arti  Arti client + onion service + onion addresses
                        └── flint-gateway   loopback gateway / standalone relay (flint-relay)
                        ▲
flint-messenger-wallet ──┘  wallet-facing service: seed derivation, encrypted
                           history/contacts/read markers, poll/event index
```

The CLI, the web UI and the wallet daemon implement the same `HttpConnector`
over `flint-tor-arti::TorHttpClient`; the wire format and delivery semantics do
not depend on the shell.

Dependencies are one-directional:

- `flint-messenger` — no wallet/node/UI; generic crates plus the Nostr
  dependencies.
- `flint-tor-arti` — arti 0.43 / tor-* 0.43, no `flint-messenger` and no grin.
- `flint-gateway` — relay/gateway: it binds its own loopback TCP listener and
  stores envelopes; no Tor, no wallet, no external HTTP client.
- `flint-messenger-cli` — links all three and implements `HttpConnector` over
  `flint-tor-arti::TorHttpClient` (`src/main.rs`).
- `flint-messenger-ui` — optional local web UI: a small blocking HTTP server
  (std `TcpListener`) with a JSON API and one embedded HTML page; it uses the
  same library API and adds no new dependencies.
- `flint-messenger-wallet` — wallet integration layer: `WalletService`
  (identity from a wallet-derived seed, encrypted history/contacts/read markers,
  poll/event index) and `flintmsg-walletd`, a loopback HTTP sidecar with bearer
  token. It reuses `flint-messenger`, `flint-gateway` and `flint-tor-arti`; no
  wallet RPC or transaction logic is linked in.

## Direct mode (primary)

```text
Alice flintmsg send                      Bob flintmsg serve
  Identity(seed→address/onion)            Gateway loopback 127.0.0.1:P
  envelope v2 (seal+mine_pow; sender proof in ciphertext)  ──►   OnionService::start(seed, P)
  Arti TorHttpClient POST                 onion:80 → 127.0.0.1:P
  http://<bob.onion>/flint/v1/msg         poll loop: GET /flint/v1/msg
                                          envelope open → print → signed ACK
```

- The onion is the only public endpoint; the gateway listens on loopback.
- The payload reaches the recipient's mailbox; the sender does not need a
  directory — the onion is derived from the address.
- Direct delivery requires the recipient to be online; TTL bounds storage.
- The network layer is in-process Arti: `tor.exe`, `torrc`, the Tor Expert Bundle
  and an external SOCKS proxy are not required.

## Relay mode

```text
Alice flintmsg send --relay http://<relay>/  ──►  flint-relay (Gateway)
Bob   flintmsg fetch --relay http://<relay>/  ◄──  mailbox files/TTL/quotas
```

- `flint-relay` — `flint-gateway` with `require_signed_ack = true`,
  `require_pow = true`, `require_fetch_proof = true`; messages are opaque
  envelopes. Mailbox reads require the address key's signature over
  address+timestamp, so knowing an address does not expose message count/size/
  time/id metadata. `--allow-unsigned-fetch` is a legacy/dev escape hatch.
- The relay can be published as an onion service (through Arti or external Tor)
  or run on loopback for tests (`--plain` in the CLI).
- ACKs are signed by the recipient key (`AckProof`); nobody can delete someone
  else's message. Message ids are canonicalized (lowercase hyphenated UUID) for
  every store/ACK operation, so letter case cannot create or address a second
  entry.

## CLI (`flintmsg`)

| Command | Purpose |
|---|---|
| `keygen` | create an identity file (32-byte seed, network) and print address/onion |
| `address` | show the address and onion for an identity file |
| `serve` | gateway + onion (Arti), print incoming JSON lines, signed ACK |
| `send` | envelope → direct recipient onion through Arti, or `--relay URL` (Arti or `--plain` loopback) |
| `fetch` | fetch/open/acknowledge messages once from a relay or loopback gateway |

The CLI deliberately has no persistent history/contacts: it is a demonstration
and diagnostics tool. The library `History`/`ContactBook`/`FileMailbox` are
available for embedding.

## Web UI (`flintmsg-ui`)

Single-user, loopback-only front-end. The server keeps the same runtime state as
`serve`/`send` and exposes a small JSON API; the page is one embedded file
(`src/ui.html`) with no external assets.

| Endpoint | Purpose |
|---|---|
| `GET /` | embedded page |
| `GET /api/state` | identity, mode, transport status, contacts, last 200 messages |
| `POST /api/send` | `{to,text}` → sealed envelope sent through relay or direct onion |
| `POST /api/contacts`, `POST /api/contacts/remove` | contact book (`contacts.json`) |
| `GET /api/export` | transcript JSON (`grinmessenger-transcript-v1`) |
| `GET /api/health` | liveness |

The UI persists `history.json` and `contacts.json` in `--data` as owner-only
files (`0600` on Unix); incoming envelopes are stored before their signed ACK is
sent. Transport startup (Arti bootstrap / onion service) runs in a background
thread so the page is available immediately and shows `tor`/`ready`/`error`
state.

Security guard (audit GM-1): the API is not "trusted because it is loopback".
Every start prints `UI_URL=http://127.0.0.1:PORT/#token=…`; the token is required
in `X-Flint-UI-Token` for all `/api/*` calls except `/api/health`, the `Host`
header must be an exact bound loopback name (defeats DNS rebinding), a present
`Origin` must be same-origin, and POST bodies must be JSON. The page embeds no
token and CSP forbids external resources.

## Wallet integration (`flint-messenger-wallet` / `flintmsg-walletd`)

Two entry points share one service implementation:

- **Embedded**: a Rust wallet calls `WalletService::open` with a 32-byte
  messaging seed and `start_network`, then uses typed methods (`send_text`,
  `events`, `contacts`, …).
- **Sidecar**: `flintmsg-walletd` owns one identity and exposes an authenticated
  loopback JSON API (`/v1/status`, `/v1/events`, `/v1/send`, …). The wallet owns
  the seed (file/env/identity file), reads the printed API URL + token file and
  supervises the process.

Key points:

- The wallet supplies the seed. `derive_messaging_seed` performs HKDF-SHA256
  domain separation; the messenger address is the same `bech32(ed25519)`
  format as a Grin slatepack address, and storage keys are derived separately.
- History, contacts and read markers are encrypted at rest with the storage key.
- The service polls the relay/onion, records opened envelopes **before** sending
  the signed ACK, and keeps a monotonic local `seq` cursor plus a per-start
  `epoch` for wallet notification loops.
- Slatepack armor is an opaque payload (`MessageKind::Slatepack`, encrypted=true
  enforced); the layer never parses or creates transactions and has no wallet
  RPC endpoint.
- The sidecar binds loopback only, requires a bearer token for every endpoint
  except `/v1/health`, and has no CORS/TLS (local process API).
- The optional wallet web console (`flintmsg-wallet-ui`) is a separate process:
  it serves one page and proxies `/api/v1/*` to the daemon, injecting the bearer
  token server-side (the token never reaches the browser). It enforces the same
  loopback `Host` allowlist, same-origin `Origin`, and JSON POST body rules as
  `flintmsg-ui`; the daemon API itself remains the only authority.

## Trust boundaries

- **Envelope**: confidentiality and integrity are provided by
  ChaCha20-Poly1305 + ed25519 signature; the relay is not trusted and is not
  needed for authentication (the recipient verifies the signature after
  decryption).
- **Relay**: sees `to`, size and time, can delete/duplicate; it does not see the
  payload/sender and cannot forge the signature. In envelope **v2** the sender's
  authentication proof is inside the ciphertext, so a relay cannot test a known
  candidate address against the public wire; relay mode hides the sender from a
  third-party relay. Legacy v1 envelopes still carry a public sender signature
  and retain the candidate-check leak (`docs/SECURITY.md`).
- **Tor/Arti**: transport confidentiality; the application does not rely on Tor
  for end-to-end encryption (the envelope provides it).
- **CLI/UI**: the identity file is a secret; CLI stdout and the UI page may
  contain plaintext messages — that is user-facing output, not service logging.
  The UI is loopback-only and requires the per-start session token plus
  Host/Origin checks.
- **Wallet daemon**: the seed never leaves the process; the loopback API is
  protected by a bearer token (`0600` file on Unix); history/contacts/read markers
  are encrypted with a separate storage key. The daemon has no wallet RPC and
  cannot spend funds. A local process that reads the token file can send
  messages as the wallet identity — treat the token like a session secret.

## Flows and resources

- One Arti runtime per process (Tokio); the onion service and `TorHttpClient`
  share one cached client for the same `TorConfig`.
- The onion service lives while `OnionService` is alive (Drop stops it).
- CLI `serve` is a blocking poll loop (2 s by default). Unacknowledged envelopes
  are stored in the file-backed mailbox of the loopback gateway
  (`<data>/mailbox`); the CLI keeps no separate decrypted history.
- Relay limits: TTL/quotas/sizes live in `GatewayConfig`; relay mode keeps the
  post-rate limiter, while direct-mode gateways disable it (every Tor sender is
  127.0.0.1, so a shared bucket was a cheap inbound DoS) and use a larger queue
  (500/recipient, 5000 total, 32 KiB payload, 12 h TTL) drained by the poll/ACK
  loop plus PoW. A sustained direct-mode flood can still delay delivery; this is
  documented residual risk, not claimed away.
- The web UI is a thin shell: one OS thread per HTTP connection, one background
  poll thread, no async runtime of its own; it reuses the Arti runtime kept by
  `flint-tor-arti`.
- The wallet daemon is the same shape: per-connection threads, one network poll
  thread, a `Mutex`-protected event/message index with long-poll windows; the
  HTTP sidecar and embedded `WalletService` use identical code paths.
- HTTP on the wire: `TorHttpClient` sends an origin-form request target with an
  explicit `Host` header, and the gateway also normalizes absolute-form targets
  (regression-tested). This matters: hyper emits absolute-form when the request
  URI carries a scheme+authority, and a minimal origin server would otherwise
  answer 404 for every `/flint/v1/*` call. Mailbox fetches from clients add a
  signed `ts`/`sig` proof; the gateway verifies it before listing metadata.
- Wallet API limits: text 8 KiB, Slatepack armor 32 KiB, TTL 1 s–7 days,
  contact limit 500, event long-poll ≤30 s.

## Out of scope

- Wallet, Grin node, RPC/owner API, Slatepack creation/finalization and their
  automatic processing. The wallet integration layer only transports the armor
  the wallet provides; transaction logic stays in the wallet.
- The Slint desktop wallet UI and the Flint desktop lifecycle (the separate
  local web UI above is included).
- Groups, attachments, ratchet/forward secrecy, offline guarantees, public abuse
  control.
- A custom P2P protocol/network.
- Native mobile/FFI builds (the wallet layer is a Rust crate + local sidecar; iOS
  needs a separate staticlib/FFI integration).

## Build and evidence

- The offline build uses the vendor at `C:\DHW\vendor` through the inherited
  `C:\DHW\.cargo\config.toml`; another machine can use plain `cargo build`
  (crates.io) with `Cargo.lock`.
- Recorded runs: `evidence/20261107-extraction/` (build, tests, loopback e2e,
  live Tor — PASS/FAIL/NOT VERIFIED).
