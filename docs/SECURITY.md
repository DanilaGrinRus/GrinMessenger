# Security hardening and audit triage (2026-11-08)

This page records the external audit findings against GrinMessenger, what was
fixed, and what remains as a documented v1 limitation. It is intentionally
explicit: a PASS elsewhere in this repository does not override a limitation
listed here.

## Fixed

### 1. `flintmsg-ui`: CSRF / DNS rebinding (was HIGH)

The local UI holds the identity and decrypted history. A web page in the same
browser could reach `http://127.0.0.1:<ui-port>` (DNS rebinding) or send a
cross-site request, read `/api/export` and send messages as the user.

Mitigations now implemented in `flint-messenger-ui`:

- per-start 32-byte session token, printed only in the URL **fragment**
  (`UI_URL=http://127.0.0.1:PORT/#token=…`). The fragment is not sent to the
  server; the page stores the token in `sessionStorage` and sends it in
  `X-Flint-UI-Token` for every `/api/*` call except `/api/health`;
- `Host` must be one of the exact bound loopback names (`127.0.0.1:PORT`,
  `localhost:PORT`, `[::1]:PORT`), which defeats DNS rebinding;
- a present `Origin` must be `http://` + an allowed host; POST bodies must be
  `application/json` (blocks CORS-simple form posts);
- non-loopback binds are refused; CSP/X-Frame-Options/no-referrer are sent.

Tests: `api_is_guarded_against_cross_site_and_rebinding` (foreign/missing Host,
missing/wrong token, `/api/export` without token, foreign Origin, text/plain).
Demo log: `evidence/20261107-extraction/30-security-demo.log`.

### 2. Relay mailbox metadata read by anyone (was HIGH)

`GET /flint/v1/msg?to=<address>` used to return message count/sizes/timestamps/
ids to anyone who knew the address.

Now the relay requires a **signed fetch proof** by default
(`flint-relay` sets `require_fetch_proof = true`):

```text
GET /flint/v1/msg?to=<address>&ts=<unix>&sig=<hex ed25519>
signature = sign("flint-gateway-fetch-v1" || 0 || address || 0 || ts)
```

The timestamp must be within ±5 minutes. Clients in this repository sign
automatically (`Transport::fetch_signed`, used by `Messenger::fetch_open`, the
CLI, the web UI and the wallet daemon); `GatewayTransport::fetch` remains the
unsigned legacy path for local gateways/tests. Old v1 clients talking to a
hardened relay get `401` until updated. The relay also keeps TTL, quotas and
per-recipient storage limits, so an address holder only sees its own mailbox.

Tests: `fetch_requires_a_signed_address_proof_when_enabled`, plus the real
`relay_live` process test. Demo log:
`evidence/20261107-extraction/30-security-demo.log`.

### 3. UUID case could address a second relay entry (MEDIUM)

`AABB…` and `aabb…` are the same UUID but were treated as different strings for
store/ACK on case-sensitive storage; a relay could be made to keep duplicates or
fail an ACK depending on the platform.

Now every POST, ACK route and stored/returned id is canonicalized with
`Uuid::parse_str(..).hyphenated().to_string()`; the PoW check compares UUIDs, not
strings, and an ACK with any letter case deletes the one canonical entry.

Tests: `mixed_case_message_ids_address_the_same_relay_entry`; the real relay
live test covers persistent storage.

### 4. Secret files written before permissions were set (LOW/MEDIUM)

Affected: wallet sidecar (tokens, generated identity seeds), CLI seed file, UI
data files.

- wallet crate: `write_secret_file` now opens with mode `0600` on Unix *before*
  writing, tightens a pre-existing file before writing, and on Windows creates
  the file then removes ACE inheritance and grants only the current user (plus
  SYSTEM) via `icacls`, failing closed if ACL hardening fails;
- `flintmsg-ui`: identity/history/contacts use owner-only `0600` files on Unix;
- `flintmsg` CLI: `identity.json` is created/opened `0600` before the seed is
  written (Windows: ACL inherited from the user profile directory; the wallet
  daemon performs explicit ACL hardening).

Tests: `secret_file_is_restricted_before_content_is_written` (wallet),
`identity_seed_file_is_owner_only` (CLI).

### 5. `flintmsg-wallet-ui` console needed the same browser guard (new code)

The wallet web console was added after the audit. It is loopback-only and
proxies `/api/v1/*` with the daemon token injected server-side (the token never
reaches the browser), but any page in the same browser could otherwise attempt
DNS rebinding/CSRF against it. It now enforces:

- exact loopback `Host` allowlist (bind address + `localhost`, exact port),
- same-origin `Origin` only, JSON content type for POST,
- token file remains server-side; the page cannot read it,
- non-loopback `--api` targets are refused.

Tests: `console_is_guarded_against_dns_rebinding_and_csrf`,
`console_refuses_unsafe_configuration`.

## Mitigated, with residual risk

### 6. Direct-mode inbound flood (was HIGH)

In direct mode the recipient's onion service forwards every sender from
`127.0.0.1`. The old direct gateway used a shared per-recipient limit of
30 messages/minute; since PoW is only 18 bits, an attacker could burn the bucket
with ~10 s of CPU per minute and block legitimate incoming messages.

Mitigation in this release:

- direct-mode gateways (`flintmsg serve`, web UI, wallet daemon) set
  `enforce_post_rate = false`: there is no shared rate bucket to hog (per-IP
  limiting is meaningless behind Tor);
- their queue is larger (500 messages/recipient, 5 000 total), spam is drained by
  the normal poll/open/ACK loop, and every message still pays proof of work;
- relay mode keeps the rate limiter, where the relay sees real peers.

Residual: an attacker who sustains a flood can still consume bandwidth/CPU and
fill the queue for a while; there is no sender identity at the gateway to build
a per-sender quota, and no recipient allowlist in v1. A queued legitimate
message can be delayed under sustained flood. This is an inherent cost of
anonymous push delivery without sender-authenticated quotas; a recipient-side
contact policy/v2 ticket design is future work.

Test: `direct_mode_queue_does_not_share_the_relay_post_rate`.

## Documented v1 limitations (not fixed in this release)

### 7. Relay can test whether a known address signed an envelope (MEDIUM)

Envelope v1 carries an outer ed25519 signature by the sender key. A relay
(anyone who sees the envelope) can take a candidate sender address (ed25519
public key), recompute the envelope transcript and verify the signature. This
confirms that the candidate sent a message of that size at that time to the
recipient, even though sender/recipient/payload are otherwise encrypted.

Why not fixed here: moving sender authentication inside the encrypted payload is
a protocol change (envelope v2 + mixed-version relays/clients, new vectors). It
is the correct direction, not a patch. Recommendations for now:

- for sender-relay unlinkability use **direct mode** (no relay sees the
  envelope; only the recipient/onion service does);
- use a relay you control or a trusted one for relay mode;
- treat "relay cannot read payload" as confidentiality, **not** sender
  anonymity. This is now stated in `docs/PROTOCOL.md` and the README.

### 8. Address-key reuse links messenger/onion to the payment address (MEDIUM, conditional)

If a wallet exports the seed of its *address key* and the messenger uses it, the
messenger address equals the wallet's slatepack address and its onion is derived
from the same key. An observer who knows the published slatepack address can see
when that onion is online, correlating payment and messaging activity.

Therefore:

- the recommended integration derives a **dedicated messaging seed** with HKDF
  (`derive_messaging_seed`) and shows a separate messenger address;
- reusing the wallet address key is no longer recommended in
  `docs/WALLET_INTEGRATION.md` (it remains technically possible for wallets that
  explicitly accept the linkability);
- never publish the wallet's slatepack address as the messenger address if
  online-time unlinkability matters.

## Verification

- `cargo test --workspace` — 154 passed, 0 failed, 2 ignored (see
  `29-security-tests.log` in the evidence directory; ignore count is the live
  Nostr test and the real-relay process test, both run separately);
- `cargo test -p flint-gateway --test relay_live -- --ignored` — PASS with the
  hardened relay;
- demo: `30-security-demo.log` (real relay fetch proof, UI guard, wallet ACL);
- the UI and wallet console security tests are in the same test suite.

## Reporting

Please report a suspected vulnerability privately to the repository owner
rather than in a public issue. This project is a DEV/testnet tool, not audited,
and mainnet use is at the caller's risk.
