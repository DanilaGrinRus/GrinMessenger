// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! flintmsg-ui — a local web UI (loopback) for GrinMessenger.
//!
//! The Rust side is a small blocking HTTP server with a JSON API; the page is a
//! single embedded HTML file (no CDN, no Node.js, no WebView dependency).
//! Two modes:
//!
//! * relay: fetch/send through a `flint-relay` (plain TCP or through in-process
//!   Arti when `--plain` is not given);
//! * direct: publish an onion service through in-process Arti, fetch incoming
//!   messages from a loopback gateway and deliver outgoing envelopes to the
//!   recipient's onion.
//!
//! The server refuses non-loopback binds. It is a single-user local tool, and
//! because any web page in the user's browser can reach loopback, the API is
//! guarded against cross-site requests and DNS rebinding:
//!
//! * a random per-start session token (printed as the `#token=` fragment of
//!   `UI_URL`, never sent in a URL to the server) is required in the
//!   `X-Flint-UI-Token` header for every `/api/*` call except `/api/health`;
//! * the `Host` header must name the bound loopback address
//!   (`127.0.0.1:PORT`, `localhost:PORT` or `[::1]:PORT`), and a present
//!   `Origin` must be `http://` + one of those hosts;
//! * `POST` bodies must be `application/json` (no CORS-"simple" form/text posts).
//!
//! The custom header alone already forces a CORS preflight that this server
//! never answers, so cross-origin scripts cannot call the API even if they
//! learn the port.

#![forbid(unsafe_code)]

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use flint_gateway::gateway::{wait_ready, Gateway, GatewayConfig};
use flint_messenger::transport::{GatewayTransport, HttpConnector, TcpConnector};
use flint_messenger::{
    Address, Identity, MessageKind, Messenger, Transport, DEFAULT_TTL_SECS, MAX_TTL_SECS,
};
use flint_tor_arti::{OnionService, TorConfig, TorHttpClient};

const UI_HTML: &str = include_str!("ui.html");
const MAX_BODY: usize = 1024 * 1024;
const MAX_MESSAGES: usize = 1000;
const STATE_MESSAGES: usize = 200;
const MAX_TEXT: usize = 8192;
/// Header that carries the per-start session token.
const TOKEN_HEADER: &str = "x-flint-ui-token";
/// Optional fixed token for supervisors/tests (at least 32 URL-safe chars).
const TOKEN_ENV: &str = "FLINTMSG_UI_TOKEN";

// ---------- options ----------

struct Options {
    identity: PathBuf,
    network: String,
    data_dir: PathBuf,
    listen: String,
    relay: Option<String>,
    plain: bool,
    tor_state: Option<PathBuf>,
    poll_secs: u64,
}

fn usage() -> &'static str {
    "flintmsg-ui — local web UI for GrinMessenger

USAGE:
  flintmsg-ui [--identity <file>] [--network tgrin|grin] [--data <dir>]
              [--listen 127.0.0.1:0] [--relay <url>] [--plain]
              [--tor-state <dir>] [--poll-secs 2]

OPTIONS:
  --identity <file>   identity JSON; created inside --data when missing
  --network <name>    network for a newly created identity (tgrin default)
  --data <dir>        data directory (history.json, contacts.json, mailbox)
  --listen <addr>     loopback bind address (default 127.0.0.1:0);
                      non-loopback addresses are refused
  --relay <url>       relay mode; without it the UI runs a direct Arti onion
  --plain             relay/cleartext transport (no Tor) for --relay
  --tor-state <dir>   Arti state directory
  --poll-secs <n>     inbox poll interval in seconds (default 2)
  --help              show this help

The server prints machine-readable lines on startup:
  UI_URL=http://127.0.0.1:PORT/#token=…   (open this exact URL)
  ADDRESS=tgrin1…
  ONION=…onion

Security: the API requires the session token from UI_URL in the
X-Flint-UI-Token header, a loopback Host header and JSON request bodies.
A fresh token is generated on every start; set FLINTMSG_UI_TOKEN (at least
32 characters of [A-Za-z0-9_-]) to pin it for a supervisor.
"
}

fn parse_args() -> Result<Options, String> {
    let mut identity: Option<PathBuf> = None;
    let mut network = "tgrin".to_string();
    let mut data_dir = PathBuf::from("flintmsg-ui-data");
    let mut listen = "127.0.0.1:0".to_string();
    let mut relay: Option<String> = None;
    let mut plain = false;
    let mut tor_state: Option<PathBuf> = None;
    let mut poll_secs = 2u64;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--identity" | "-i" => {
                i += 1;
                identity = Some(PathBuf::from(
                    args.get(i).ok_or("--identity: missing value")?,
                ));
            }
            "--network" => {
                i += 1;
                network = args.get(i).ok_or("--network: missing value")?.clone();
            }
            "--data" => {
                i += 1;
                data_dir = PathBuf::from(args.get(i).ok_or("--data: missing value")?);
            }
            "--listen" => {
                i += 1;
                listen = args.get(i).ok_or("--listen: missing value")?.clone();
            }
            "--relay" => {
                i += 1;
                relay = Some(args.get(i).ok_or("--relay: missing value")?.clone());
            }
            "--plain" => plain = true,
            "--tor-state" => {
                i += 1;
                tor_state = Some(PathBuf::from(
                    args.get(i).ok_or("--tor-state: missing value")?,
                ));
            }
            "--poll-secs" => {
                i += 1;
                poll_secs = args
                    .get(i)
                    .ok_or("--poll-secs: missing value")?
                    .parse()
                    .map_err(|_| "--poll-secs: expected a number".to_string())?;
            }
            "--help" | "-h" => {
                println!("{}", usage());
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {other:?}; see --help")),
        }
        i += 1;
    }
    if !["tgrin", "testnet", "grin", "mainnet"].contains(&network.as_str()) {
        return Err(format!("--network {network:?}: expected tgrin or grin"));
    }
    let data_dir = data_dir;
    Ok(Options {
        identity: identity.unwrap_or_else(|| data_dir.join("identity.json")),
        network,
        data_dir,
        listen,
        relay,
        plain,
        tor_state,
        poll_secs: poll_secs.max(1),
    })
}

// ---------- identity / persistence ----------

#[derive(Serialize, Deserialize)]
struct IdentityFile {
    version: u8,
    network: String,
    seed_hex: String,
}

fn network_hrp(network: &str) -> &'static str {
    match network {
        "grin" | "mainnet" => "grin",
        _ => "tgrin",
    }
}

fn load_or_create_identity(path: &Path, network: &str) -> Result<(Identity, [u8; 32]), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("identity dir: {e}"))?;
    }
    let (seed, network) = if path.exists() {
        let text =
            fs::read_to_string(path).map_err(|e| format!("identity {}: {e}", path.display()))?;
        let file: IdentityFile =
            serde_json::from_str(&text).map_err(|e| format!("identity JSON: {e}"))?;
        if file.version != 1 {
            return Err(format!(
                "identity version {}: only 1 is supported",
                file.version
            ));
        }
        let bytes = data_encoding::HEXLOWER
            .decode(file.seed_hex.trim().as_bytes())
            .map_err(|e| format!("identity seed_hex: {e}"))?;
        let seed: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| "identity seed_hex: expected 32 bytes".to_string())?;
        (seed, file.network)
    } else {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed).map_err(|e| format!("RNG: {e}"))?;
        let file = IdentityFile {
            version: 1,
            network: network.to_string(),
            seed_hex: data_encoding::HEXLOWER.encode(&seed),
        };
        let text =
            serde_json::to_string_pretty(&file).map_err(|e| format!("identity JSON: {e}"))?;
        write_private_file(path, format!("{text}\n").as_bytes(), true)
            .map_err(|e| format!("identity {}: {e}", path.display()))?;
        (seed, network.to_string())
    };
    let identity =
        Identity::from_seed(seed, network_hrp(&network)).map_err(|e| format!("identity: {e}"))?;
    Ok((identity, seed))
}

#[derive(Clone, Serialize, Deserialize)]
struct Contact {
    name: String,
    address: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct MessageRecord {
    id: String,
    direction: String,
    peer: String,
    kind: String,
    text: String,
    created_at: u64,
    pow_bits: u8,
    ttl: u64,
    route: String,
}

/// Write a file readable only by the owner on Unix (`0600`). With `create_new`
/// an existing file is never overwritten (the identity seed must not be
/// clobbered by a racing second process). On Windows the file inherits the
/// ACL of the data directory, so keep `--data` inside the user's profile.
fn write_private_file(path: &Path, bytes: &[u8], create_new: bool) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true);
    if create_new {
        options.create_new(true);
    } else {
        options.create(true).truncate(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_vec_pretty(value).map_err(|e| format!("json: {e}"))?;
    // History and contacts hold plaintext messages: keep them owner-only. The
    // mode applies only on creation, so drop a stale temp file first.
    let _ = fs::remove_file(&tmp);
    write_private_file(&tmp, &text, false).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))?;
    Ok(())
}

fn load_json<T>(path: &Path) -> T
where
    T: for<'de> Deserialize<'de> + Default,
{
    let Ok(text) = fs::read_to_string(path) else {
        return T::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

// ---------- runtime state ----------

type FetchSlot = (String, Arc<dyn HttpConnector>);
type SendSlot = (Option<String>, Arc<dyn HttpConnector>);

struct Shared {
    identity: Identity,
    seed: [u8; 32],
    address: String,
    onion: String,
    network: String,
    data_dir: PathBuf,
    tor_state: PathBuf,
    relay_url: Option<String>,
    plain: bool,
    listen: String,
    mode_label: String,
    /// Per-start session token required by `/api/*` (see module docs).
    token: String,
    history: Mutex<Vec<MessageRecord>>,
    contacts: Mutex<Vec<Contact>>,
    status: Mutex<Status>,
    /// (base url, connector) used to fetch the inbox.
    fetch: Mutex<Option<FetchSlot>>,
    /// (fixed remote or None for direct per-recipient, connector) used to send.
    send: Mutex<Option<SendSlot>>,
    onion_service: Mutex<Option<OnionService>>,
    gateway: Mutex<Option<Gateway>>,
}

#[derive(Clone, Default)]
struct Status {
    phase: String,
    detail: String,
    fetch_ready: bool,
    send_ready: bool,
    last_error: Option<String>,
    last_poll_unix: u64,
}

struct ArtiConnector {
    client: TorHttpClient,
}

impl HttpConnector for ArtiConnector {
    fn request(
        &self,
        method: &str,
        url: &str,
        body: &[u8],
    ) -> flint_messenger::Result<(u16, Vec<u8>)> {
        self.client
            .request(method, url, body)
            .map_err(|e| flint_messenger::Error::Store(format!("arti: {e}")))
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn history_path(shared: &Shared) -> PathBuf {
    shared.data_dir.join("history.json")
}

fn contacts_path(shared: &Shared) -> PathBuf {
    shared.data_dir.join("contacts.json")
}

fn persist_history(shared: &Shared) -> Result<(), String> {
    let records = lock(&shared.history).clone();
    save_json(&history_path(shared), &records)
}

fn persist_contacts(shared: &Shared) -> Result<(), String> {
    let contacts = lock(&shared.contacts).clone();
    save_json(&contacts_path(shared), &contacts)
}

fn set_status(shared: &Shared, phase: &str, detail: impl Into<String>) {
    let mut status = lock(&shared.status);
    status.phase = phase.to_string();
    status.detail = detail.into();
}

fn set_error(shared: &Shared, error: impl Into<String>) {
    let message = error.into();
    eprintln!("flintmsg-ui: {message}");
    let mut status = lock(&shared.status);
    status.phase = "error".to_string();
    status.detail = message.clone();
    status.last_error = Some(message);
}

fn set_ready(shared: &Shared, detail: impl Into<String>) {
    let mut status = lock(&shared.status);
    status.phase = "ready".to_string();
    status.detail = detail.into();
    status.fetch_ready = true;
    status.send_ready = true;
    status.last_error = None;
}

fn route_label(shared: &Shared) -> String {
    if shared.relay_url.is_some() {
        "relay".to_string()
    } else {
        "direct".to_string()
    }
}

// ---------- networking ----------

fn init_network(shared: Arc<Shared>) {
    let mut tor_config = TorConfig::new(shared.tor_state.clone());
    tor_config.bootstrap_timeout = Duration::from_secs(150);
    tor_config.request_timeout = Duration::from_secs(60);
    // The relay URL is fixed by CLI options; direct mode derives the remote from
    // the recipient address. `--plain` disables Tor for relay mode.
    let relay = shared.relay_url.clone();
    match relay {
        Some(url) => {
            if shared.plain {
                let connector: Arc<dyn HttpConnector> = Arc::new(TcpConnector::new());
                *lock(&shared.fetch) = Some((url.clone(), connector.clone()));
                *lock(&shared.send) = Some((Some(url.clone()), connector));
                set_ready(&shared, format!("relay {url} (plain TCP)"));
            } else {
                set_status(&shared, "tor", "Arti bootstrap for relay transport…");
                match TorHttpClient::bootstrap(&tor_config) {
                    Ok(client) => {
                        let connector: Arc<dyn HttpConnector> = Arc::new(ArtiConnector { client });
                        *lock(&shared.fetch) = Some((url.clone(), connector.clone()));
                        *lock(&shared.send) = Some((Some(url.clone()), connector));
                        set_ready(&shared, format!("relay {url} over Arti"));
                    }
                    Err(e) => set_error(&shared, format!("Tor/Arti bootstrap: {e}")),
                }
            }
        }
        None => {
            set_status(
                &shared,
                "tor",
                "starting loopback gateway and Arti onion service…",
            );
            match start_direct(&shared, &tor_config) {
                Ok(()) => {
                    set_ready(&shared, format!("direct onion {}.onion", shared.onion));
                }
                Err(e) => set_error(&shared, e),
            }
        }
    }
}

fn start_direct(shared: &Shared, tor_config: &TorConfig) -> Result<(), String> {
    let mailbox = shared.data_dir.join("mailbox");
    fs::create_dir_all(&mailbox).map_err(|e| format!("mailbox: {e}"))?;
    let gateway = Gateway::start(GatewayConfig {
        bind: "127.0.0.1:0".parse().expect("bind"),
        store_dir: Some(mailbox),
        require_signed_ack: true,
        require_pow: true,
        ttl_secs: 12 * 60 * 60,
        max_per_recipient: 500,
        max_total: 5_000,
        max_payload: 32 * 1024,
        max_posts_per_minute: 30,
        // All Tor senders share 127.0.0.1, so a shared rate bucket is a cheap
        // inbound DoS. Direct mode uses the larger queue + PoW + poll/ack.
        enforce_post_rate: false,
        ..GatewayConfig::default()
    })
    .map_err(|e| format!("gateway: {e}"))?;
    if !wait_ready(&gateway, Duration::from_secs(5)) {
        return Err("gateway did not pass the health check".into());
    }
    let gateway_url = gateway.url();
    let service = OnionService::start(&shared.seed, &gateway.addr().to_string(), tor_config)
        .map_err(|e| format!("onion service: {e}"))?;
    let client =
        TorHttpClient::bootstrap(tor_config).map_err(|e| format!("Tor/Arti bootstrap: {e}"))?;
    // Keep the gateway and the onion service alive for the whole process.
    *lock(&shared.gateway) = Some(gateway);
    *lock(&shared.onion_service) = Some(service);
    // Incoming messages arrive into the loopback gateway over plain TCP; outgoing
    // envelopes go directly to the recipient onion through Arti.
    *lock(&shared.fetch) = Some((
        gateway_url,
        Arc::new(TcpConnector::new()) as Arc<dyn HttpConnector>,
    ));
    *lock(&shared.send) = Some((
        None,
        Arc::new(ArtiConnector { client }) as Arc<dyn HttpConnector>,
    ));
    Ok(())
}

fn poll_loop(shared: Arc<Shared>, poll_secs: u64) {
    loop {
        thread::sleep(Duration::from_secs(poll_secs));
        let Some((url, connector)) = lock(&shared.fetch).clone() else {
            continue;
        };
        let transport = match GatewayTransport::with_connector(&url, connector) {
            Ok(t) => t,
            Err(e) => {
                set_error(&shared, format!("transport: {e}"));
                continue;
            }
        };
        let now = flint_messenger::envelope::unix_now();
        let envelopes = match transport.fetch_signed(&shared.identity, now, 50) {
            Ok(v) => v,
            Err(e) => {
                let mut status = lock(&shared.status);
                status.last_error = Some(format!("fetch: {e}"));
                continue;
            }
        };
        if envelopes.is_empty() {
            let mut status = lock(&shared.status);
            status.last_poll_unix = now;
            status.last_error = None;
            continue;
        }
        let mut ack_ids: Vec<String> = Vec::new();
        let mut changed = false;
        let route = route_label(&shared);
        for envelope in envelopes {
            let opened = match envelope.open(&shared.identity, now) {
                Ok(opened) => opened,
                Err(e) => {
                    let mut status = lock(&shared.status);
                    status.last_error = Some(format!("rejected envelope {}: {e}", envelope.id));
                    continue;
                }
            };
            let mut history = lock(&shared.history);
            if !history.iter().any(|record| record.id == opened.id) {
                let (kind, text) = match &opened.kind {
                    MessageKind::Text { text } => ("text".to_string(), text.clone()),
                    MessageKind::Slatepack { armor, .. } => {
                        ("slatepack".to_string(), armor.clone())
                    }
                };
                history.push(MessageRecord {
                    id: opened.id.clone(),
                    direction: "in".to_string(),
                    peer: opened.sender.to_string(),
                    kind,
                    text,
                    created_at: opened.created_at,
                    pow_bits: envelope.pow_bits,
                    ttl: envelope.ttl,
                    route: route.clone(),
                });
                if history.len() > MAX_MESSAGES {
                    let overflow = history.len() - MAX_MESSAGES;
                    history.drain(0..overflow);
                }
                changed = true;
            }
            drop(history);
            ack_ids.push(opened.id);
        }
        if changed {
            if let Err(e) = persist_history(&shared) {
                set_error(&shared, e);
            }
        }
        if !ack_ids.is_empty() {
            match transport.ack_signed(&shared.identity, &ack_ids) {
                Ok(_) => {
                    let mut status = lock(&shared.status);
                    status.last_poll_unix = now;
                    status.last_error = None;
                }
                Err(e) => {
                    let mut status = lock(&shared.status);
                    status.last_error = Some(format!("ACK: {e}"));
                }
            }
        }
    }
}

// ---------- send ----------

fn handle_send(shared: &Shared, body: &Value) -> Result<Value, String> {
    let to_raw = body
        .get("to")
        .and_then(Value::as_str)
        .ok_or("send: \"to\" is required")?
        .trim();
    let recipient: Address = to_raw
        .parse()
        .map_err(|e: flint_messenger::Error| format!("send: {e}"))?;
    let text = body
        .get("text")
        .and_then(Value::as_str)
        .ok_or("send: \"text\" is required")?
        .to_string();
    if text.trim().is_empty() {
        return Err("send: empty text".into());
    }
    if text.len() > MAX_TEXT {
        return Err(format!("send: text is longer than {MAX_TEXT} bytes"));
    }
    let (remote, connector) = {
        let send = lock(&shared.send).clone();
        let Some((fixed, connector)) = send else {
            return Err("transport is not ready yet (see status)".into());
        };
        let remote = match fixed {
            Some(url) => url,
            None => recipient.onion_url(),
        };
        (remote, connector)
    };
    let transport = GatewayTransport::with_connector(&remote, connector)
        .map_err(|e| format!("transport: {e}"))?;
    let messenger = Messenger::new(transport);
    let kind = MessageKind::Text { text: text.clone() };
    let ttl = DEFAULT_TTL_SECS.min(MAX_TTL_SECS);
    let envelope = messenger
        .send(&shared.identity, &recipient, kind, ttl)
        .map_err(|e| format!("send: {e}"))?;
    let record = MessageRecord {
        id: envelope.id.clone(),
        direction: "out".to_string(),
        peer: recipient.to_string(),
        kind: "text".to_string(),
        text,
        created_at: envelope.created_at,
        pow_bits: envelope.pow_bits,
        ttl: envelope.ttl,
        route: route_label(shared),
    };
    {
        let mut history = lock(&shared.history);
        history.push(record);
        if history.len() > MAX_MESSAGES {
            let overflow = history.len() - MAX_MESSAGES;
            history.drain(0..overflow);
        }
    }
    persist_history(shared)?;
    Ok(json!({
        "ok": true,
        "id": envelope.id,
        "remote": remote,
    }))
}

fn handle_add_contact(shared: &Shared, body: &Value) -> Result<Value, String> {
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let address_raw = body
        .get("address")
        .and_then(Value::as_str)
        .ok_or("contact: \"address\" is required")?
        .trim()
        .to_string();
    let address: Address = address_raw
        .parse()
        .map_err(|e: flint_messenger::Error| format!("contact: {e}"))?;
    if name.chars().count() > 64 {
        return Err("contact: name is longer than 64 characters".into());
    }
    if name.chars().any(char::is_control) {
        return Err("contact: name contains control characters".into());
    }
    let name = if name.is_empty() {
        address.to_string()
    } else {
        name
    };
    {
        let mut contacts = lock(&shared.contacts);
        if contacts.iter().any(|c| c.address == address.to_string()) {
            return Err("contact: address already exists".into());
        }
        contacts.push(Contact {
            name,
            address: address.to_string(),
        });
    }
    persist_contacts(shared)?;
    Ok(json!({ "ok": true }))
}

fn handle_remove_contact(shared: &Shared, body: &Value) -> Result<Value, String> {
    let address = body
        .get("address")
        .and_then(Value::as_str)
        .ok_or("contact: \"address\" is required")?;
    let before = lock(&shared.contacts).len();
    lock(&shared.contacts).retain(|contact| contact.address != address);
    let after = lock(&shared.contacts).len();
    persist_contacts(shared)?;
    Ok(json!({ "ok": true, "removed": before != after }))
}

// ---------- HTTP API ----------

fn state_json(shared: &Shared) -> Value {
    let status = lock(&shared.status).clone();
    let contacts = lock(&shared.contacts).clone();
    let mut messages: Vec<MessageRecord> = {
        let history = lock(&shared.history);
        history
            .iter()
            .rev()
            .take(STATE_MESSAGES)
            .rev()
            .cloned()
            .collect()
    };
    if messages.len() > STATE_MESSAGES {
        messages.drain(0..messages.len() - STATE_MESSAGES);
    }
    let (fetch_url, gateway_url) = {
        let fetch = lock(&shared.fetch).clone();
        let gateway = lock(&shared.gateway).as_ref().map(Gateway::url);
        (fetch.map(|(url, _)| url), gateway)
    };
    json!({
        "ok": true,
        "identity": {
            "address": shared.address,
            "onion": shared.onion,
            "onion_url": format!("http://{}.onion", shared.onion),
            "network": shared.network,
        },
        "mode": {
            "kind": if shared.relay_url.is_some() { "relay" } else { "direct" },
            "plain": shared.plain,
            "relay": shared.relay_url,
            "gateway": gateway_url,
            "fetch_url": fetch_url,
            "listen": shared.listen,
            "label": shared.mode_label,
        },
        "status": {
            "phase": status.phase,
            "detail": status.detail,
            "fetch_ready": status.fetch_ready,
            "send_ready": status.send_ready,
            "last_error": status.last_error,
            "last_poll_unix": status.last_poll_unix,
        },
        "contacts": contacts,
        "messages": messages,
        "counts": {
            "messages": lock(&shared.history).len(),
            "contacts": lock(&shared.contacts).len(),
        },
    })
}

fn export_json(shared: &Shared) -> Value {
    json!({
        "format": "grinmessenger-transcript-v1",
        "exported_at": flint_messenger::envelope::unix_now(),
        "identity": {
            "address": shared.address,
            "onion": shared.onion,
            "network": shared.network,
        },
        "contacts": lock(&shared.contacts).clone(),
        "messages": lock(&shared.history).clone(),
    })
}

fn route(shared: &Shared, method: &str, path: &str, body: &[u8]) -> (u16, &'static str, Vec<u8>) {
    let json = |status: u16, value: Value| -> (u16, &'static str, Vec<u8>) {
        (
            status,
            "application/json; charset=utf-8",
            serde_json::to_vec(&value).unwrap_or_else(|_| b"{\"ok\":false}".to_vec()),
        )
    };
    let parse = |body: &[u8]| -> Result<Value, (u16, &'static str, Vec<u8>)> {
        serde_json::from_slice(body).map_err(|e| {
            (
                400,
                "application/json; charset=utf-8",
                serde_json::to_vec(&json!({"ok": false, "error": format!("json: {e}")}))
                    .unwrap_or_default(),
            )
        })
    };
    if method == "GET" && (path == "/" || path == "/index.html") {
        return (200, "text/html; charset=utf-8", UI_HTML.as_bytes().to_vec());
    }
    if method == "GET" && path == "/favicon.ico" {
        return (204, "image/x-icon", Vec::new());
    }
    if method == "GET" && path == "/api/health" {
        return json(200, json!({"ok": true}));
    }
    if method == "GET" && path == "/api/state" {
        return json(200, state_json(shared));
    }
    if method == "GET" && path == "/api/export" {
        return json(200, export_json(shared));
    }
    if method == "POST" && path == "/api/send" {
        let value = match parse(body) {
            Ok(v) => v,
            Err(response) => return response,
        };
        return match handle_send(shared, &value) {
            Ok(value) => json(200, value),
            Err(message) => json(400, json!({"ok": false, "error": message})),
        };
    }
    if method == "POST" && path == "/api/contacts" {
        let value = match parse(body) {
            Ok(v) => v,
            Err(response) => return response,
        };
        return match handle_add_contact(shared, &value) {
            Ok(value) => json(200, value),
            Err(message) => json(400, json!({"ok": false, "error": message})),
        };
    }
    if method == "POST" && path == "/api/contacts/remove" {
        let value = match parse(body) {
            Ok(v) => v,
            Err(response) => return response,
        };
        return match handle_remove_contact(shared, &value) {
            Ok(value) => json(200, value),
            Err(message) => json(400, json!({"ok": false, "error": message})),
        };
    }
    if path.starts_with("/api/") {
        return json(404, json!({"ok": false, "error": "unknown endpoint"}));
    }
    (404, "text/plain; charset=utf-8", b"not found".to_vec())
}

struct Request {
    method: String,
    path: String,
    /// Header names are lower-cased; values are trimmed.
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<Request> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut data: Vec<u8> = Vec::new();
    let mut buffer = [0u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "empty request",
            ));
        }
        data.extend_from_slice(&buffer[..read]);
        if let Some(position) = find_bytes(&data, b"\r\n\r\n") {
            break position + 4;
        }
        if data.len() > 64 * 1024 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "request headers too large",
            ));
        }
    };
    let head = String::from_utf8_lossy(&data[..header_end]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default().to_string();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let full_path = parts.next().unwrap_or("/").to_string();
    let path = full_path.split('?').next().unwrap_or("/").to_string();
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                content_length = value.parse().unwrap_or(0);
            }
            headers.push((name, value));
        }
    }
    if content_length > MAX_BODY {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "request body too large",
        ));
    }
    let mut body = data[header_end.min(data.len())..].to_vec();
    while body.len() < content_length {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buffer[..read]);
        if body.len() > MAX_BODY {
            break;
        }
    }
    body.truncate(content_length.min(MAX_BODY));
    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

// ---------- request guard (CSRF / DNS rebinding) ----------

/// `Host` values accepted for a listener bound to `addr`. Only the exact port
/// is accepted, so a rebinding attacker's own hostname never matches.
fn allowed_hosts(addr: std::net::SocketAddr) -> Vec<String> {
    let port = addr.port();
    let mut hosts = vec![format!("localhost:{port}")];
    match addr.ip() {
        std::net::IpAddr::V4(ip) => hosts.push(format!("{ip}:{port}")),
        std::net::IpAddr::V6(ip) => hosts.push(format!("[{ip}]:{port}")),
    }
    hosts
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

/// Reject a request before routing. Returns `(status, error)` on rejection.
fn guard(request: &Request, token: &str, hosts: &[String]) -> Option<(u16, &'static str)> {
    // DNS rebinding: the browser sends the attacker's hostname in Host.
    let host_ok = request
        .header("host")
        .map(|host| {
            hosts
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(host))
        })
        .unwrap_or(false);
    if !host_ok {
        return Some((403, "host not allowed"));
    }
    // Cross-site requests: browsers attach Origin to POST and to CORS fetches.
    if let Some(origin) = request.header("origin") {
        let origin_ok = hosts
            .iter()
            .any(|allowed| origin.eq_ignore_ascii_case(&format!("http://{allowed}")));
        if !origin_ok {
            return Some((403, "origin not allowed"));
        }
    }
    if !request.path.starts_with("/api/") || request.path == "/api/health" {
        return None;
    }
    let token_ok = request
        .header(TOKEN_HEADER)
        .map(|provided| constant_time_eq(provided.as_bytes(), token.as_bytes()))
        .unwrap_or(false);
    if !token_ok {
        return Some((401, "missing or invalid session token"));
    }
    if request.method == "POST" {
        let json = request
            .header("content-type")
            .and_then(|value| value.split(';').next())
            .map(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
            .unwrap_or(false);
        if !json {
            return Some((415, "content-type must be application/json"));
        }
    }
    None
}

fn session_token() -> Result<String, String> {
    if let Ok(token) = std::env::var(TOKEN_ENV) {
        let token = token.trim().to_string();
        let valid = token.len() >= 32
            && token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if !valid {
            return Err(format!(
                "{TOKEN_ENV}: expected at least 32 characters of [A-Za-z0-9_-]"
            ));
        }
        return Ok(token);
    }
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|e| format!("RNG: {e}"))?;
    Ok(data_encoding::HEXLOWER.encode(&bytes))
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        415 => "Unsupported Media Type",
        500 => "Internal Server Error",
        _ => "Response",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n\
         X-Frame-Options: DENY\r\n\
         Referrer-Policy: no-referrer\r\n\
         Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

fn handle_connection(mut stream: TcpStream, shared: Arc<Shared>, hosts: Arc<Vec<String>>) {
    let result = (|| -> std::io::Result<()> {
        let request = match read_request(&mut stream) {
            Ok(request) => request,
            Err(_) => {
                return write_response(
                    &mut stream,
                    400,
                    "application/json; charset=utf-8",
                    b"{\"ok\":false,\"error\":\"bad request\"}",
                );
            }
        };
        let allowed = matches!(request.method.as_str(), "GET" | "POST");
        if !allowed {
            return write_response(
                &mut stream,
                405,
                "application/json; charset=utf-8",
                b"{\"ok\":false,\"error\":\"method not allowed\"}",
            );
        }
        if let Some((status, error)) = guard(&request, &shared.token, &hosts) {
            let body =
                serde_json::to_vec(&json!({"ok": false, "error": error})).unwrap_or_default();
            return write_response(
                &mut stream,
                status,
                "application/json; charset=utf-8",
                &body,
            );
        }
        let (status, content_type, response_body) =
            route(&shared, &request.method, &request.path, &request.body);
        write_response(&mut stream, status, content_type, &response_body)
    })();
    if let Err(error) = result {
        eprintln!("flintmsg-ui: connection error: {error}");
    }
}

fn serve(shared: Arc<Shared>) -> Result<(), String> {
    let listener =
        TcpListener::bind(&shared.listen).map_err(|e| format!("bind {}: {e}", shared.listen))?;
    let addr = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {e}"))?;
    if !addr.ip().is_loopback() {
        return Err(format!(
            "refusing to listen on non-loopback address {addr}: the UI holds the identity and \
             plaintext history and must stay local"
        ));
    }
    let hosts = Arc::new(allowed_hosts(addr));
    println!("UI_URL=http://{}/#token={}", hosts[1], shared.token);
    println!("ADDRESS={}", shared.address);
    println!("ONION={}.onion", shared.onion);
    println!("MODE={}", shared.mode_label);
    println!("DATA={}", shared.data_dir.display());
    let _ = std::io::stdout().flush();
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                let shared = Arc::clone(&shared);
                let hosts = Arc::clone(&hosts);
                thread::spawn(move || handle_connection(stream, shared, hosts));
            }
            Err(error) => {
                eprintln!("flintmsg-ui: accept: {error}");
                thread::sleep(Duration::from_millis(200));
            }
        }
    }
    Ok(())
}

fn main() {
    let options = match parse_args() {
        Ok(options) => options,
        Err(error) => {
            eprintln!("flintmsg-ui: {error}");
            std::process::exit(2);
        }
    };
    if let Err(error) = fs::create_dir_all(&options.data_dir) {
        eprintln!(
            "flintmsg-ui: data dir {}: {error}",
            options.data_dir.display()
        );
        std::process::exit(1);
    }
    let (identity, seed) = match load_or_create_identity(&options.identity, &options.network) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("flintmsg-ui: {error}");
            std::process::exit(1);
        }
    };
    let token = match session_token() {
        Ok(token) => token,
        Err(error) => {
            eprintln!("flintmsg-ui: {error}");
            std::process::exit(2);
        }
    };
    let address = identity.address().to_string();
    let onion = flint_tor_arti::onion_address(&seed);
    let mode_label = match &options.relay {
        Some(url) => {
            if options.plain {
                format!("relay {url} (plain TCP)")
            } else {
                format!("relay {url} over Arti")
            }
        }
        None => format!("direct onion {onion}.onion"),
    };
    let history: Vec<MessageRecord> = load_json(&options.data_dir.join("history.json"));
    let contacts: Vec<Contact> = load_json(&options.data_dir.join("contacts.json"));
    let tor_state = options
        .tor_state
        .clone()
        .unwrap_or_else(|| options.data_dir.join("arti-state"));
    let shared = Arc::new(Shared {
        identity,
        seed,
        address,
        onion,
        network: options.network.clone(),
        data_dir: options.data_dir.clone(),
        tor_state,
        relay_url: options.relay.clone(),
        plain: options.plain,
        listen: options.listen.clone(),
        mode_label,
        token,
        history: Mutex::new(history),
        contacts: Mutex::new(contacts),
        status: Mutex::new(Status {
            phase: "init".to_string(),
            detail: "starting…".to_string(),
            ..Status::default()
        }),
        fetch: Mutex::new(None),
        send: Mutex::new(None),
        onion_service: Mutex::new(None),
        gateway: Mutex::new(None),
    });
    let network_shared = Arc::clone(&shared);
    let poll_shared = Arc::clone(&shared);
    let poll_secs = options.poll_secs;
    thread::spawn(move || init_network(network_shared));
    thread::spawn(move || poll_loop(poll_shared, poll_secs));
    if let Err(error) = serve(shared) {
        eprintln!("flintmsg-ui: {error}");
        std::process::exit(1);
    }
}
