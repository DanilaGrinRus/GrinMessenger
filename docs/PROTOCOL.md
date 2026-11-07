# GrinMessenger — messenger protocol v1 (standalone)

Status: **extracted and adapted implementation of Flint messenger v1**; build
and test verification lives in `evidence/20261107-extraction/`. This is not an
approved standard and not part of Grin consensus. Wallet integration, automatic
Slatepack processing (S1/S2), groups, attachments, ratchet and a public relay
service are out of scope for this project.

The workspace contains four crates:

- `flint-messenger` — the protocol: addresses, envelope, cryptography, PoW, ACK,
  store/history/contacts, transports (`GatewayTransport`, `DirectOnionTransport`,
  Nostr);
- `flint-tor-arti` — in-process Tor: onion v3 derived from the same key plus an
  HTTP client through Arti;
- `flint-gateway` — a standalone store-and-forward relay without a wallet, plus
  the `flint-relay` binary;
- `flint-messenger-cli` — the `flintmsg` binary (keygen/address/serve/send/fetch).

The envelope v1 wire format, the signature/AAD/PoW domains and the delivery rules
are preserved from Flint messenger v1; adaption of storage formats and CLI glue
is explicitly allowed by the owner (see `PROVENANCE.md`).

## 1. Identity and address

The identity is a 32-byte ed25519 seed. There is exactly one long-term key:

1. `SigningKey::from_bytes(seed)` → ed25519 public key;
2. the address is bech32(`hrp`, public key), `hrp` = `tgrin` (testnet) or `grin`
   (mainnet), i.e. the slatepack address format;
3. the Tor v3 onion is derived from the same key:
   `base32(PUBKEY || SHA3-256(".onion checksum" || PUBKEY || 0x03)[0..2] || 0x03)`;
   no separate onion address is needed.

The CLI keeps the seed in an identity file:

```json
{"version": 1, "network": "tgrin", "seed_hex": "<64 hex characters>"}
```

The file is a secret: do not publish it or copy it into reports. History recovery
and device migration are the user's responsibility (v1 has no separate chat
key-management/recovery; losing the seed means losing the identity).

## 2. Envelope

JSON; binary fields are base64:

| Field | Type / meaning |
|---|---|
| `version` | u8, currently 1 |
| `id` | UUID, delivery and ACK identifier |
| `recipient` | recipient address (bech32) |
| `created_at` | Unix seconds |
| `ttl` | lifetime in seconds |
| `ephemeral_key` | 32-byte X25519 public key of the sender |
| `nonce` | 12-byte ChaCha20-Poly1305 nonce |
| `ciphertext` | encrypted payload |
| `pow_nonce` / `pow_bits` | Proof-of-work; 0 for legacy envelopes |
| `signature` | 64-byte ed25519 sender signature |

The payload inside the ciphertext contains: `sender`, `type` = `text` |
`slatepack`, `text` or `armor`, `encrypted=true` for slatepack, and `created_at`.
Plaintext slatepack (`encrypted=false`) is refused on both `seal` and `open`.
The sender and the type are not carried as separate plaintext fields, but this is
not full sender concealment (see §9).

## 3. Cryptography

- Ed25519 seed → `SHA-512(seed)[..32]` → X25519 scalar; the public key is derived
  through the Edwards→Montgomery map.
- The sender creates an ephemeral X25519 key and performs DH with the recipient's
  public key.
- `HKDF-SHA256(salt = recipient X25519 public, info = "flint-messenger/v1/encryption")`
  produces a 32-byte key.
- ChaCha20-Poly1305; the AAD binds `version`, `id`, `recipient`, `created_at` and
  `ttl` using length-prefixed strings.
- A domain-separated ed25519 transcript binds those fields, `ephemeral_key`,
  `nonce`, the length and the ciphertext; for PoW envelopes it also binds
  `pow_nonce`/`pow_bits`.
- SHA-256 PoW binds `id`/`recipient`/`created_at`/`ttl`, the ciphertext commitment
  and the nonce.
- After decryption the recipient sees the sender and verifies the signature with
  the sender's address key. The relay cannot forge the signature but can check a
  known sender candidate: the signature and transcript are public (a metadata
  leak of v1; fixing it would require a new protocol).
- No custom cryptography: the project uses established libraries
  (`ed25519-dalek`, `x25519-dalek`, `chacha20poly1305`, `hkdf`, `sha2`, `sha3`,
  `argon2`, `ring`).

## 4. Proof of work

`Envelope::seal` mines PoW immediately:

- `POW_BITS` = 20 (release) / 12 (debug), `POW_MIN_BITS` = 18 (release) / 12
  (debug), maximum 24;
- `pow_bits=0` is a legacy envelope; a public gateway with `require_pow` rejects
  it, while `open` still reads legacy envelopes for compatibility;
- a relay/gateway with `require_pow` checks the difficulty before storing and the
  recipient checks it before decryption.

PoW only limits spam; it is not a proof of identity.

## 5. Delivery, TTL and the gateway API

Delivery is at-least-once: a retry delivers the same signed envelope with the
same `id`. TTL is enforced by both the mailbox and the recipient;
`DEFAULT_TTL_SECS = 93600` (26 h), `MAX_TTL_SECS = 604800` (7 days). A specific
gateway may enforce a stricter limit and clamps the stored message TTL.

| API | Body / result |
|---|---|
| `POST /flint/v1/msg` | `{id,to,payload,ttl_secs}` → `{id,expires_at}` |
| `GET /flint/v1/msg?to=address&limit=N` | messages `id/to/payload/created_at/expires_at`, without `from` |
| `POST /flint/v1/msg/<uuid>/ack` | `{to,id,ts,signature}`; recipient signature, otherwise 401/404 |
| `GET /flint/v1/health` | `{ok:true}` |

Rules:

- `id` is a UUID; path traversal is rejected;
- the ACK transcript binds `to/id/ts`; the time window is ±300 s; knowing an
  address does not grant the right to delete someone else's message;
- relay/onion listeners require a signed ACK; an unsigned "legacy" ACK is
  accepted only by a local test gateway
  (`GatewayConfig.require_signed_ack = false`);
- relay defaults: up to 100 messages per recipient, payload ≤ 64 KiB, 10000 in
  total; `flint-relay` additionally enables `require_pow` and
  `require_signed_ack`;
- responses are bounded before reading/allocating; quotas account for the
  persisted store after restart; overfilling one recipient does not consume
  another recipient's quota; a storage failure never reports false success;
- logs contain no message texts or secrets;
- server and client need reasonably synchronized clocks (for ACK).

## 6. Transports

### 6.1 Direct onion through Arti (primary)

- The recipient (`flintmsg serve`) starts a loopback gateway and publishes an
  onion through in-process Arti (no `tor.exe`); the gateway is the only target of
  the onion.
- The sender builds the envelope and sends `POST /flint/v1/msg` to
  `http://<recipient-onion>.onion` through the Arti HTTP client.
- The recipient fetches messages from its loopback gateway, opens them, prints
  them to the user and signs an ACK.
- Direct delivery requires the recipient to be online: if its onion/gateway is
  unreachable, the message is not delivered. The CLI does not keep an outbox or
  retries.

### 6.2 Relay (store-and-forward)

- `flint-relay` is a standalone relay without a wallet. Both sides may reach it
  through Arti (`.onion`) or locally (`--plain` for tests).
- The relay sees only the opaque payload, the recipient address, the size and
  timestamps. It cannot read the content or forge the signature.
- An offline recipient fetches messages on its next run (within TTL).

### 6.3 Compatible options in the library

- `GatewayTransport::new(url)` — plain HTTP (loopback/tests).
- `GatewayTransport::with_socks(url, socks)` and
  `DirectOnionTransport::new(local, socks)` — delivery through an external Tor
  SOCKS5 proxy.
- `HttpConnector`/`TcpConnector` — the extension point: the CLI implements an
  Arti connector on top of `flint-tor-arti`, so the same gateway protocol works
  over in-process Tor without changing the wire format.
- The Nostr transport (NIP-44/NIP-59) is present in `flint-messenger` as a
  separate delivery option, but the v1 CLI does not enable it and there is no
  silent fallback between transports.

## 7. Local storage (library)

The extracted modules keep their original semantics, but the CLI does not use
them (v1 CLI prints messages to stdout and keeps no persistent history):

- `FileMailbox` — signed encrypted envelopes only, atomic writes, TTL pruning,
  quotas, signed ACK;
- `History` — ChaCha20-Poly1305 under a 32-byte profile key, AAD = `peer+id`, a
  fresh nonce per record;
- `ContactBook` — addresses/names under the history key, atomic writes, up to 500
  contacts, name ≤ 64 characters;
- history key: per network/profile, keywrap v2 — Argon2id (64 MiB, t=3, p=4) +
  ChaCha20-Poly1305; v1 PBKDF2-HMAC-SHA256 (100000) is read and repackaged into
  v2; whoever owns the seed/password owns the history.

## 8. Transport exchange and ACK

- The recipient durably records the received message before signing an ACK; an
  ACK error does not hide already opened messages.
- A repeated/corrupted envelope is quarantined/tombstoned locally instead of
  breaking the whole batch; a blocked sender never reaches history.
- An ACK confirms delivery, not a payment: this standalone project performs no
  monetary operations.

## 9. Security and limits

- No forward secrecy: a long-term recipient key allows decrypting old traffic
  after compromise. Groups, attachments, ratchet, Sealed Sender, push and
  distributed abuse control are outside v1.
- The relay can delay/delete/duplicate messages and correlate recipient, size and
  time; it cannot read the payload or forge a verifiable signature.
- Direct connections expose the transport-level Tor guard path, not application
  metadata; Tor does not hide all metadata.
- One onion per chat identity; the wallet payment onion is not used and is not
  mixed with the chat in this project.
- Application logging must not contain plaintext/seed/keys; the CLI prints only
  addresses, onion names, ids and the messages themselves (stdout is user
  output).
- The project does not promise anonymity, absence of bugs or mainnet readiness;
  it is a DEV/testnet tool.

## 10. Compatibility

The envelope v1 wire format and the gateway API (`/flint/v1/msg*`) are
compatible with Flint messenger v1; known fixed vectors (encryption, signature,
PoW, ACK) were carried over into unit tests. Local formats (CLI identity file,
store) may differ from Flint and are not part of it. The payment Slatepack
scenarios of the original Flint (automatic handoff to a wallet) are not included:
`MessageKind::Slatepack` is carried as opaque armor without wallet checks.
