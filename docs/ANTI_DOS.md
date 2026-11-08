# Direct-mode inbound DoS: analysis and mitigation

Status: implemented incremental mitigation + design notes for the remaining
research problem. Related: `docs/SECURITY.md` §6.

## Threat model

Direct mode exposes a recipient onion service. Tor forwards every inbound
connection to the local gateway from `127.0.0.1`, so the gateway cannot tell
senders apart by IP and cannot read the sender (envelope v2 keeps sender
authentication inside the ciphertext). An attacker who knows the onion address:

1. solves the envelope proof-of-work (~18 bits release, cheap CPU);
2. POSTs valid envelopes addressed to the victim;
3. tries to exhaust the victim's queue/rate capacity so legitimate messages are
   rejected with `429` or delayed.

Existing cryptography prevents the attacker from reading/forging messages but
cannot, by itself, prevent storage/CPU/bandwidth consumption.

## Constraints

- No sender identity is visible to the gateway in v2. A sender-visible quota
  would either reintroduce sender deanonymization (outer identity) or require
  anonymous credentials/blind tickets.
- All senders arrive from loopback; per-IP quotas are meaningless.
- PoW is spam pricing, not authentication; it can be raised but legitimate
  senders pay too.
- The recipient's client is online by definition in direct mode and can drain
  the inbox quickly (poll + open + signed ACK).

## Implemented mitigation (this release)

1. **Removed the shared loopback post-rate bucket** (the old 30/min per
   recipient). With all Tor senders behind 127.0.0.1 it let one cheap attacker
   hold the bucket and block everyone: the original HIGH finding. Direct
   gateways set `enforce_post_rate = false`.
2. **Larger bounded queue**: 500 messages/recipient, 5 000 total, TTL and
   payload limits unchanged. Queue capacity is a buffer, not a rate policy.
3. **Fast recipient drain**: direct-mode clients poll every 1 s with
   `fetch_limit = 100` (CLI/UI/wallet defaults). A message is opened, recorded
   and ACKed, so the queue drains at up to ~100 messages/s.
4. **Proof of work remains required** by the direct gateway (`require_pow`).
5. **Envelope v2** keeps sender authentication inside the payload, so this
   mitigation does not trade away sender anonymity.

Cost sketch (release): 18-bit PoW is roughly 0.1–0.3 s of CPU per message on a
modern core. Filling the 500-message queue costs on the order of 1–2 CPU-minutes;
sustaining rejection above the 100 msg/s drain needs ~100+ cores of PoW plus
network. A short burst can still add latency, but a cheap 10 s/min attack no
longer blocks legitimate delivery. Exact numbers depend on hardware.

## Options considered and not chosen now

| Option | Why not (yet) |
|---|---|
| Per-sender quota via outer sender address | Returns the v1 deanonymization; conflicts with envelope v2. |
| Per-IP quota | Useless behind Tor (everything is 127.0.0.1). |
| Static high PoW (e.g. 24 bits) | Punishes legitimate senders and battery/mobile users; attacker can still rent CPU. |
| Contacts-only inbox | Breaks first contact; needs a UI/policy and bootstrap exchange. Could be offered as an opt-in later. |
| Recipient-issued blind tickets | Promising: recipient issues unlinkable single-use permits, gateway verifies them without learning the sender. Requires a ticket handshake, storage, rate policy for ticket issuance, and new tests; larger protocol/API change. |
| Dynamic PoW challenge | Gateway raises difficulty under pressure and clients retry; attacker can pay, and it adds retry/UX complexity. Could combine with tickets. |
| Deposit/fee | Not acceptable for a free messenger. |

## Operations guidance

- Put the direct gateway behind the onion only; never expose its loopback port.
- Keep TTL and queue limits bounded; monitor `last_error`/queue pressure if you
  operate a long-lived identity.
- If the address is being flooded, rotate the identity (new seed) and re-share
  the new address; this is the practical escape hatch in v1/v2.
- Relay mode pushes the problem to the relay operator, which can apply per-IP
  quotas and abuse controls; direct mode cannot.

## Future work

1. Contact policy: an explicit contacts-only mode with a quarantine for unknown
   senders, plus UI for accepting them.
2. Blind-ticket inbox: recipient-issued permits with PoW priced issuance;
   gateway checks the ticket, not the sender identity, and one ticket = one
   message.
3. Dynamic PoW watermark with client retry and a `required_pow_bits` hint in the
   gateway response.

Until then, the residual risk is documented honestly: a **sustained** direct-mode
flood can still delay delivery; there is no per-sender authenticated quota in
this release.
