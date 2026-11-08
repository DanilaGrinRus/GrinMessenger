# Relay deployment kit

This directory contains a minimal, security-first deployment for a public
GrinMessenger store-and-forward relay. The relay is wallet-agnostic: it only
stores opaque, signed, encrypted envelopes with TTL, quotas, proof-of-work and
signed fetch/ACK proofs.

**Read this first:** a public relay is an abuse surface and a privacy-sensitive
service. Run it on a host you control, publish it only as a Tor v2/v3 onion (or
behind TLS + a reverse proxy you trust), and keep the plaintext listener on
loopback. The project is DEV/testnet software and not audited.

## Recommended topology

```text
Internet ──Tor──► onion:80 ──► 127.0.0.1:18080 (flint-relay)
                                   │
                                   └── /var/lib/flint-relay/store (opaque files)
```

- `flint-relay` listens on loopback only.
- Tor publishes it as a hidden service (no inbound firewall port).
- Do not put the relay behind a cleartext public port. If you must expose it,
  terminate TLS and add rate limiting/abuse controls at the reverse proxy.
- No wallet, node or RPC is involved.

## Build and install (Linux, systemd + Tor)

Tested layout (Debian/Ubuntu):

```bash
sudo useradd --system --home /var/lib/flint-relay --shell /usr/sbin/nologin flint
sudo install -d -o flint -g flint -m 0750 /var/lib/flint-relay/store

git clone https://github.com/DanilaGrinRus/GrinMessenger.git
cd GrinMessenger
cargo build --release --locked -p flint-gateway --bin flint-relay
sudo install -o root -g root -m 0755 target/release/flint-relay /usr/local/bin/flint-relay

sudo install -m 0644 deploy/relay/systemd/flint-relay.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now flint-relay
curl -fsS http://127.0.0.1:18080/flint/v1/health   # {"ok":true}
```

Tor hidden service (`/etc/tor/torrc`):

```text
HiddenServiceDir /var/lib/tor/flint-relay/
HiddenServicePort 80 127.0.0.1:18080
```

```bash
sudo systemctl restart tor
sudo cat /var/lib/tor/flint-relay/hostname   # abc…xyz.onion
```

Share that onion address with your test users. Keep the relay binary updated by
re-running the build/install and `systemctl restart flint-relay`.

## Configuration

`systemd/flint-relay.service` uses:

| Flag | Value | Why |
|---|---|---|
| `--listen` | `127.0.0.1:18080` | loopback only; Tor reaches it |
| `--store` | `/var/lib/flint-relay/store` | persistent opaque queue |
| `--ttl` | `93600` (26 h) | bounds disk retention |
| `--max-per` | `100` | per-recipient queue bound |
| `--max-total` | `10000` | global bound |
| signed acks | required (default) | only recipient can delete its mail |
| PoW | required (default) | spam cost |
| signed fetch proof | required (default) | mailbox metadata only to address holder |

The relay prints no message content. See `docs/PROTOCOL.md` §5 for the API and
`docs/SECURITY.md` for the trust model.

## Monitoring and maintenance

- liveness: `curl -fsS http://127.0.0.1:18080/flint/v1/health`
- disk: `df -h /var/lib/flint-relay`; `du -sh /var/lib/flint-relay/store`
- logs: `journalctl -u flint-relay -f` (no message bodies are logged)
- restart: `systemctl restart flint-relay` (queue persists on disk)
- backup: the store contains opaque envelopes only; a consistent backup is
  optional and expires by TTL. Do not publish the store.
- upgrade: build the new tag, install the binary, restart. Clients remain
  compatible across envelope v1/v2; fetch proofs are additive.

A simple cron check:

```cron
*/5 * * * * curl -fsS http://127.0.0.1:18080/flint/v1/health >/dev/null || systemctl restart flint-relay
```

For richer metrics, scrape the health endpoint plus host metrics; the relay
intentionally exposes no message or peer counters.

## Abuse and capacity guidance

- Enforce `require_signed_ack`, `require_pow` and `require_fetch_proof` (defaults
  in the `flint-relay` binary; do not disable them in production).
- Keep `--max-total`, `--max-per`, TTL and payload limits bounded; disk is the
  main resource.
- PoW is only spam pricing; a determined attacker can still fill a recipient's
  queue. TTL and quotas bound damage; rotate a flooded address.
- Do not add wallet/RPC endpoints to the relay. It is not a wallet service.
- Respect your jurisdiction and Tor/relay abuse policies; publish an operator
  contact where appropriate.

## Docker (optional)

`deploy/relay/Dockerfile` builds the relay from source. It is provided as an
example; a container that binds loopback is not reachable from a host Tor unless
you use `--network host` (Linux) or adjust the bind/reverse proxy accordingly.
Review the Dockerfile and pin the base image by digest for production.

```bash
docker build -f deploy/relay/Dockerfile -t grinmessenger-relay .
# Linux host-network example; Tor points at 127.0.0.1:18080
docker run --rm --network host --read-only \
  -v /var/lib/flint-relay:/data \
  grinmessenger-relay
```
