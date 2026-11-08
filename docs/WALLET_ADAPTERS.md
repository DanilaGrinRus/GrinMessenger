# Wallet adapters

This is the adapter-level guide for specific wallets. The generic contract lives
in [WALLET_INTEGRATION.md](WALLET_INTEGRATION.md); this file covers what a wallet
repository has to change, packaging and mobile.

## 1. grin-wallet (Rust): embed the service

grin-wallet is a Rust project, so it should link the library, not spawn the
sidecar.

### Repository changes (PR-ready outline)

1. Add a workspace dependency:

   ```toml
   flint-messenger-wallet = { git = "https://github.com/DanilaGrinRus/GrinMessenger", rev = "<tag>" }
   ```

   Pin a release tag (see `docs/RELEASE.md`), not `main`.

2. Add a feature flag so the base wallet stays free of messenger/Tor
   dependencies:

   ```toml
   [features]
   messenger = ["dep:flint-messenger-wallet"]
   ```

3. Create `src/messenger.rs` with a thin adapter:

   ```rust
   pub struct MessengerHandle {
       service: Arc<WalletService>,
       stop: Arc<AtomicBool>,
   }

   impl MessengerHandle {
       /// `master_seed` stays inside the wallet process; only the derived seed is
       /// passed to the messenger service.
       pub fn start(&self, data_dir: &Path, relay: Option<String>, master_seed: &[u8]) -> Result<Self> {
           let seed = derive_messaging_seed(master_seed, "grin-wallet/mainnet/account-0/messenger");
           let mode = match relay {
               Some(url) => NetworkMode::Relay { url, plain: false },
               None => NetworkMode::Direct,
           };
           let service = Arc::new(WalletService::open(ServiceConfig {
               data_dir: data_dir.to_path_buf(),
               network: "grin".into(),
               mode,
               poll_secs: 1,
               fetch_limit: 100,
               ..ServiceConfig::default()
           }, Zeroizing::new(seed))?);
           service.start_network();
           Ok(Self { service, stop: Arc::new(AtomicBool::new(false)) })
       }
   }
   ```

4. Wire it into the wallet's existing runtime/notification loop:

   - call `service.events(cursor, Duration::from_secs(25))` (or
     `messages(after, 1000)`) on a background thread;
   - on `EventKind::Message` put a notification in the wallet UI, mark unread;
   - on `MessageKind::Slatepack` hand `armor` to the wallet's existing Slatepack
     finalize path — the messenger never parses transactions;
   - `send_slatepack(peer, armor, None)` for an outgoing invoice/response;
   - `service.mark_read(peer)` when the wallet conversation is opened.

5. Persist the **messaging seed derivation label** with the wallet account
   metadata (`account-0/messenger`); never store the derived seed separately.
   Back up the wallet master seed as usual — the messenger data directory itself
   (encrypted history/contacts) is backuppable but not required for identity.

6. Configuration:

   - `--messenger-relay <url>` (optional; default direct onion, which needs Tor);
   - `--messenger-enable` (default off for a wallet release; enable with a
     feature flag/experimental config);
   - do not put a raw seed in CLI args; derive it from the wallet's unlocked
     master seed.

7. Testing (wallet repo):

   - unit: derivation label stability and address vector;
   - integration: start relay fixture, two `WalletService` instances, send/receive
     text and an opaque Slatepack armor roundtrip;
   - upgrade: v1/v2 envelope open; wallet never sends v1 after the upgrade.

### UX notes

- Show the messenger address separately from the payment slatepack address and
  explain that the payment address is not used for chat.
- Direct mode has a 30–150 s first bootstrap; show a status indicator and allow
  relay mode for always-on wallets.
- The wallet RPC is not touched by this module; the wallet decides when a
  Slatepack is needed.

## 2. Grin++/C++/Qt: ship the sidecar

Grin++ should spawn two processes: `flintmsg-walletd` (messenger API) and, if
the user chooses direct mode, it handles Tor internally (no external tor.exe).
The wallet UI talks HTTP to loopback.

### Process handshake

1. Wallet generates/derives the dedicated messaging seed (`derive_messaging_seed`
   equivalent, HKDF-SHA256 with
   `salt="flint-messenger/wallet-integration/v1"`, info label per account) and
   writes it to a temporary file (or passes `--seed-env`).
2. Start `flintmsg-walletd --data <wallet-data>/messenger --seed-file <tmp> --network grin --relay <url>`.
   Delete the temp seed file as soon as the daemon has read it.
3. Read the first stdout line (JSON): `api`, `token_file`, `address`, `mode`.
4. Read the token file; **never** log it. Send
   `Authorization: Bearer <token>` to every `/v1` endpoint except `/v1/health`.
5. Poll `GET /v1/status` until `phase == "ready"`; use `last_error` for UI.
6. Run a long-poll thread: `GET /v1/events?since=<cursor>&wait=20`; update the
   Qt model on `message` events. On `epoch` change, refetch
   `GET /v1/messages?after=0`.
7. Provide "send text" and "send Slatepack armor" actions mapped to
   `POST /v1/send`; show the returned `message` id and `unread`.
8. On wallet shutdown: terminate the daemon, delete the temp seed file and any
   copied token; the daemon data directory stays.

### Packaging checklist

- Take binaries from the GitHub Release (`v*`): Windows `.exe` and Linux/macOS
  builds are published with `SHA256SUMS-*.txt` and a CycloneDX SBOM.
- Put the sidecar next to the wallet binary or in the app bundle; make the data
  directory user-private.
- Windows: the daemon applies an explicit `icacls` ACL to token/identity files;
  on Unix it uses `0600` modes.
- Autostart only the wallet; start the daemon lazily when the Messages tab is
  opened, or keep it as a supervised child with restart-on-crash and bounded
  backoff.
- Bundle version: pin a release tag; show it in "About".
- No inbound firewall rule is needed: the daemon binds loopback; direct mode
  publishes its onion through in-process Arti.

### Security rules for the adapter

- The wallet owns the master seed; the daemon gets only the derived seed, not the
  wallet seed. Never pass the master seed through the sidecar CLI.
- Do not reuse the wallet address-key seed for the messenger (linkability:
  `docs/SECURITY.md` §8).
- Treat the token file as a session secret; don't put it in Qt logs, crash dumps
  or telemetry.
- Do not enable CORS in the daemon or proxy `/v1` through the wallet's own web
  server; the API is a local process boundary.

## 3. Mobile (iOS/Android): FFI plan (not implemented)

The sidecar is not viable on mobile: no spawning arbitrary processes, background
execution limits and app-sandbox restrictions. The plan:

1. Build `flint-messenger-wallet` as a Rust `staticlib`/`cdylib` with a small
   C ABI (`start`, `stop`, `send_text`, `send_slatepack`, `poll_events`,
   `mark_read`, `status`), generated bindings via UniFFI or a hand-written C header.
2. iOS: Swift wrapper; store the derived seed in Keychain (Secure Enclave where
   available). Android: Kotlin wrapper; store the seed in Keystore-backed
   encrypted storage.
3. Direct mode: Arti bootstrap and onion service on mobile are heavy; relay mode
   is the practical default. Background fetch is best-effort (BGTaskScheduler /
   WorkManager); the UI must handle delayed delivery and unread counters on
   foreground.
4. Slatepack armor crosses the FFI as a string; the wallet's mobile transaction
   code remains the only owner of transaction logic.
5. Threat model: mobile OS may suspend the process; no offline guarantee; no
   forward secrecy in v1/v2; push notifications would require a metadata-leaking
   third party (out of scope).

This track needs a separate design/prototype in the wallet repository; this
project provides the Rust service and the contract it must wrap.

## 4. Reference adapter artifacts

- `crates/flint-messenger-wallet/examples/embedded_wallet.rs` — compilable Rust
  embedding example.
- `docs/WALLET_INTEGRATION.md` — generic contract, API, seed handling.
- Release artifacts: <https://github.com/DanilaGrinRus/GrinMessenger/releases>.
