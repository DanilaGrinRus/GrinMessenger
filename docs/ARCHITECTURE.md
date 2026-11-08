# GrinMessenger architecture

## Crates

```text
flintmsg (CLI)  ─────┐
flintmsg-ui (web) ───┴──► flint-messenger   envelope, crypto, PoW, ACK, store, transport
                        ├── flint-tor-arti  Arti client + onion service + onion addresses
                        └── flint-gateway   loopback gateway / standalone relay (flint-relay)
```

The CLI and the web UI implement the same `HttpConnector` over
`flint-tor-arti::TorHttpClient`; the wire format and delivery semantics do not
depend on the shell.

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

## Direct mode (primary)

```text
Alice flintmsg send                      Bob flintmsg serve
  Identity(seed→address/onion)            Gateway loopback 127.0.0.1:P
  envelope v1 (seal+mine_pow+sign)  ──►   OnionService::start(seed, P)
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
  `require_pow = true`; messages are opaque envelopes.
- The relay can be published as an onion service (through Arti or external Tor)
  or run on loopback for tests (`--plain` in the CLI).
- ACKs are signed by the recipient key (`AckProof`); nobody can delete someone
  else's message.

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

The UI persists `history.json` and `contacts.json` in `--data`; incoming
envelopes are stored before their signed ACK is sent. Transport startup (Arti
bootstrap / onion service) runs in a background thread so the page is available
immediately and shows `tor`/`ready`/`error` state. There is no authentication:
the server binds loopback and must not be exposed.

## Trust boundaries

- **Envelope**: confidentiality and integrity are provided by
  ChaCha20-Poly1305 + ed25519 signature; the relay is not trusted and is not
  needed for authentication (the recipient verifies the signature after
  decryption).
- **Relay**: sees `to`, size and time, can delete/duplicate; it does not see the
  payload/sender and cannot forge the signature.
- **Tor/Arti**: transport confidentiality; the application does not rely on Tor
  for end-to-end encryption (the envelope provides it).
- **CLI/UI**: the identity file is a secret; CLI stdout and the UI page may
  contain plaintext messages — that is user-facing output, not service logging.
  The UI is loopback-only and has no authentication.

## Flows and resources

- One Arti runtime per process (Tokio); the onion service and `TorHttpClient`
  share one cached client for the same `TorConfig`.
- The onion service lives while `OnionService` is alive (Drop stops it).
- CLI `serve` is a blocking poll loop (2 s by default). Unacknowledged envelopes
  are stored in the file-backed mailbox of the loopback gateway
  (`<data>/mailbox`); the CLI keeps no separate decrypted history.
- Relay limits: TTL/quotas/sizes live in `GatewayConfig`; CLI `serve` uses
  stricter P2P limits (32 KiB payload, 30 per recipient, 12 h TTL), while
  `flint-relay` uses the relay defaults.
- The web UI is a thin shell: one OS thread per HTTP connection, one background
  poll thread, no async runtime of its own; it reuses the Arti runtime kept by
  `flint-tor-arti`.
- HTTP on the wire: `TorHttpClient` sends an origin-form request target with an
  explicit `Host` header, and the gateway also normalizes absolute-form targets
  (regression-tested). This matters: hyper emits absolute-form when the request
  URI carries a scheme+authority, and a minimal origin server would otherwise
  answer 404 for every `/flint/v1/*` call.

## Out of scope

- Wallet, Grin node, RPC/owner API, Slatepack transactions and their automatic
  processing.
- The Slint desktop wallet UI and the Flint desktop lifecycle (the separate
  local web UI above is included).
- Groups, attachments, ratchet/forward secrecy, offline guarantees, public abuse
  control.
- A custom P2P protocol/network.

## Build and evidence

- The offline build uses the vendor at `C:\DHW\vendor` through the inherited
  `C:\DHW\.cargo\config.toml`; another machine can use plain `cargo build`
  (crates.io) with `Cargo.lock`.
- Recorded runs: `evidence/20261107-extraction/` (build, tests, loopback e2e,
  live Tor — PASS/FAIL/NOT VERIFIED).
