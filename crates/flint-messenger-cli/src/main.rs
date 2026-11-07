// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! flintmsg — two-peer CLI demo for the standalone Flint messenger.
//!
//! Modes:
//! * direct: `serve` publishes an onion (in-process Arti) in front of a loopback
//!   gateway; `send` posts the signed envelope to the recipient's onion.
//! * relay: `send --relay` / `fetch --relay` use a `flint-relay` instance
//!   (through Arti, or with `--plain` for loopback tests).
//!
//! Output is one JSON object per line on stdout so scripts can consume it.
//! Errors go to stderr; exit code is 1 for runtime errors.

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

use flint_gateway::gateway::{wait_ready, Gateway, GatewayConfig};
use flint_messenger::transport::{GatewayTransport, HttpConnector, TcpConnector};
use flint_messenger::{Address, Identity, MessageKind, Messenger, DEFAULT_TTL_SECS};
use flint_tor_arti::{OnionService, TorConfig, TorHttpClient};

type CliResult<T> = Result<T, String>;

const USAGE: &str = "\
flintmsg — standalone Flint messenger (direct Tor/Arti or relay)

USAGE:
  flintmsg keygen [--out <identity.json>] [--network tgrin|grin] [--force]
  flintmsg address [--identity <file>]
  flintmsg serve --identity <file> [--data <dir>] [--listen 127.0.0.1:0]
                 [--poll-secs 2] [--limit 20] [--ttl-secs 43200]
                 [--tor-state <dir>] [--bridge-line <line>] [--bridge-bin <path>]
  flintmsg send --identity <file> --to <address> (--text <text> | --armor-file <file>)
                [--ttl-secs 93600] [--relay <url>] [--plain]
                [--data <dir>] [--tor-state <dir>]
                [--bridge-line <line>] [--bridge-bin <path>]
  flintmsg fetch --identity <file> --source <url> [--plain] [--limit 20] [--no-ack]
                 [--tor-state <dir>] [--bridge-line <line>] [--bridge-bin <path>]

Direct mode: `send` computes http://<recipient onion>.onion from the address and
posts through in-process Arti. `serve` keeps a loopback gateway behind the onion
and prints incoming messages as JSON lines.
Relay mode: pass --relay to send and --source to fetch. --plain disables Tor and
connects directly over TCP (loopback/clearnet only).

Identity files are JSON and contain a secret seed: {\"version\":1,
\"network\":\"tgrin\",\"seed_hex\":\"…\"}. Do not publish them.
";

#[derive(Serialize, Deserialize)]
struct IdentityFile {
    version: u8,
    network: String,
    seed_hex: String,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        println!("{USAGE}");
        std::process::exit(if args.is_empty() { 2 } else { 0 });
    }
    let command = args[0].as_str();
    let rest = &args[1..];
    let result = match command {
        "keygen" => cmd_keygen(rest),
        "address" => cmd_address(rest),
        "serve" => cmd_serve(rest),
        "send" => cmd_send(rest),
        "fetch" => cmd_fetch(rest),
        other => Err(format!("неизвестная команда {other:?}; см. --help")),
    };
    if let Err(error) = result {
        eprintln!("flintmsg: {error}");
        std::process::exit(1);
    }
}

// ---------- identity file ----------

fn take(args: &[String], i: &mut usize, flag: &str) -> CliResult<String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("{flag}: не хватает значения"))
}

fn network_hrp(network: &str) -> CliResult<&'static str> {
    match network {
        "tgrin" | "testnet" => Ok("tgrin"),
        "grin" | "mainnet" => Ok("grin"),
        other => Err(format!("сеть {other:?}: ожидалось tgrin или grin")),
    }
}

fn save_identity(path: &Path, network: &str, seed: &[u8; 32]) -> CliResult<()> {
    let file = IdentityFile {
        version: 1,
        network: network.to_string(),
        seed_hex: data_encoding::HEXLOWER.encode(seed),
    };
    let json = serde_json::to_string_pretty(&file).map_err(|e| format!("identity JSON: {e}"))?;
    fs::write(path, format!("{json}\n"))
        .map_err(|e| format!("identity {}: {e}", path.display()))?;
    Ok(())
}

fn load_identity(path: &Path) -> CliResult<(Identity, [u8; 32])> {
    let text = fs::read_to_string(path).map_err(|e| format!("identity {}: {e}", path.display()))?;
    let file: IdentityFile =
        serde_json::from_str(&text).map_err(|e| format!("identity JSON: {e}"))?;
    if file.version != 1 {
        return Err(format!(
            "identity version {}: поддерживается только 1",
            file.version
        ));
    }
    let hrp = network_hrp(&file.network)?;
    let bytes = data_encoding::HEXLOWER
        .decode(file.seed_hex.trim().as_bytes())
        .map_err(|e| format!("identity seed_hex: {e}"))?;
    let seed: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| "identity seed_hex: ожидались 32 байта".to_string())?;
    let identity = Identity::from_seed(seed, hrp).map_err(|e| format!("identity: {e}"))?;
    Ok((identity, seed))
}

// ---------- keys/addresses ----------

fn cmd_keygen(args: &[String]) -> CliResult<()> {
    let mut out = PathBuf::from("identity.json");
    let mut network = "tgrin".to_string();
    let mut force = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" | "-o" => out = PathBuf::from(take(args, &mut i, "--out")?),
            "--network" => network = take(args, &mut i, "--network")?,
            "--force" => force = true,
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("keygen: неизвестный аргумент {other:?}")),
        }
        i += 1;
    }
    let hrp = network_hrp(&network)?;
    if out.exists() && !force {
        return Err(format!(
            "{} уже существует; используйте --force, чтобы перезаписать",
            out.display()
        ));
    }
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|e| format!("RNG: {e}"))?;
    let identity = Identity::from_seed(seed, hrp).map_err(|e| format!("identity: {e}"))?;
    save_identity(&out, if hrp == "grin" { "grin" } else { "tgrin" }, &seed)?;
    emit_json(&json!({
        "event": "keygen",
        "identity": out.display().to_string(),
        "network": network,
        "address": identity.address().to_string(),
        "onion": onion_of(&seed),
    }));
    Ok(())
}

fn cmd_address(args: &[String]) -> CliResult<()> {
    let mut identity_path = PathBuf::from("identity.json");
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--identity" | "-i" => identity_path = PathBuf::from(take(args, &mut i, "--identity")?),
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("address: неизвестный аргумент {other:?}")),
        }
        i += 1;
    }
    let (identity, seed) = load_identity(&identity_path)?;
    let address = identity.address().clone();
    emit_json(&json!({
        "event": "address",
        "address": address.to_string(),
        "onion": onion_of(&seed),
        "onion_url": format!("http://{}.onion", onion_of(&seed)),
    }));
    Ok(())
}

fn onion_of(seed: &[u8; 32]) -> String {
    flint_tor_arti::onion_address(seed)
}

// ---------- connector wiring ----------

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

struct TorOptions {
    state_dir: PathBuf,
    bridge_line: Option<String>,
    bridge_bin: Option<PathBuf>,
}

impl TorOptions {
    fn config(&self) -> TorConfig {
        let mut config = TorConfig::new(self.state_dir.clone());
        config.bootstrap_timeout = Duration::from_secs(150);
        config.request_timeout = Duration::from_secs(60);
        config.bridge_line = self.bridge_line.clone();
        config.bridge_bin_path = self.bridge_bin.clone();
        config
    }
}

fn make_connector(plain: bool, tor: &TorOptions) -> CliResult<Arc<dyn HttpConnector>> {
    if plain {
        return Ok(Arc::new(TcpConnector::new()));
    }
    let client =
        TorHttpClient::bootstrap(&tor.config()).map_err(|e| format!("Tor/Arti bootstrap: {e}"))?;
    Ok(Arc::new(ArtiConnector { client }))
}

fn remote_url(plain: bool, url: &str) -> CliResult<String> {
    let trimmed = url.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return Err("пустой URL".into());
    }
    if plain && trimmed.contains(".onion") {
        return Err(
            "--plain не умеет .onion: direct-доставка и onion-relay требуют Arti (уберите --plain)"
                .into(),
        );
    }
    Ok(trimmed)
}

// ---------- direct/receive loop ----------

fn cmd_serve(args: &[String]) -> CliResult<()> {
    let mut identity_path = PathBuf::from("identity.json");
    let mut data = PathBuf::from("flintmsg-data");
    let mut listen = "127.0.0.1:0".to_string();
    let mut poll_secs = 2u64;
    let mut limit = 20usize;
    let mut ttl_secs = 12 * 60 * 60u64;
    let mut tor_state: Option<PathBuf> = None;
    let mut bridge_line: Option<String> = None;
    let mut bridge_bin: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--identity" | "-i" => identity_path = PathBuf::from(take(args, &mut i, "--identity")?),
            "--data" => data = PathBuf::from(take(args, &mut i, "--data")?),
            "--listen" => listen = take(args, &mut i, "--listen")?,
            "--poll-secs" => {
                poll_secs = take(args, &mut i, "--poll-secs")?
                    .parse()
                    .map_err(|_| "--poll-secs: ожидалось число".to_string())?
            }
            "--limit" => {
                limit = take(args, &mut i, "--limit")?
                    .parse()
                    .map_err(|_| "--limit: ожидалось число".to_string())?
            }
            "--ttl-secs" => {
                ttl_secs = take(args, &mut i, "--ttl-secs")?
                    .parse()
                    .map_err(|_| "--ttl-secs: ожидалось число".to_string())?
            }
            "--tor-state" => tor_state = Some(PathBuf::from(take(args, &mut i, "--tor-state")?)),
            "--bridge-line" => bridge_line = Some(take(args, &mut i, "--bridge-line")?),
            "--bridge-bin" => bridge_bin = Some(PathBuf::from(take(args, &mut i, "--bridge-bin")?)),
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("serve: неизвестный аргумент {other:?}")),
        }
        i += 1;
    }
    let (identity, seed) = load_identity(&identity_path)?;
    let mailbox = data.join("mailbox");
    fs::create_dir_all(&mailbox).map_err(|e| format!("mailbox {}: {e}", mailbox.display()))?;
    let tor = TorOptions {
        state_dir: tor_state.unwrap_or_else(|| data.join("arti-state")),
        bridge_line,
        bridge_bin,
    };

    let gateway = Gateway::start(GatewayConfig {
        bind: listen
            .parse()
            .map_err(|_| format!("--listen {listen:?}: ожидался host:port"))?,
        store_dir: Some(mailbox),
        require_signed_ack: true,
        require_pow: true,
        ttl_secs,
        max_per_recipient: 30,
        max_total: 1_000,
        max_payload: 32 * 1024,
        max_posts_per_minute: 30,
        ..GatewayConfig::default()
    })
    .map_err(|e| format!("gateway: {e}"))?;
    if !wait_ready(&gateway, Duration::from_secs(5)) {
        return Err("gateway не ответил на health-check".into());
    }
    let onion = OnionService::start(&seed, &gateway.addr().to_string(), &tor.config())
        .map_err(|e| format!("onion service: {e}"))?;
    emit_json(&json!({
        "event": "ready",
        "address": identity.address().to_string(),
        "onion": onion.onion(),
        "onion_url": format!("http://{}.onion", onion.onion()),
        "gateway": gateway.url(),
    }));

    let transport = GatewayTransport::new(&gateway.url())
        .map_err(|e| format!("local gateway transport: {e}"))?;
    let messenger = Messenger::new(transport);
    let mut acked: HashSet<String> = HashSet::new();
    loop {
        match messenger.fetch_open(&identity, limit) {
            Ok(batch) => {
                let mut pending: Vec<String> = Vec::new();
                for id in &batch.duplicates {
                    if !acked.contains(id) {
                        pending.push(id.clone());
                    }
                }
                for opened in &batch.opened {
                    let body = match &opened.kind {
                        MessageKind::Text { text } => json!({"kind": "text", "text": text}),
                        MessageKind::Slatepack { armor, encrypted } => {
                            json!({"kind": "slatepack", "armor": armor, "encrypted": encrypted})
                        }
                    };
                    let mut object = json!({
                        "event": "message",
                        "id": opened.id,
                        "from": opened.sender.to_string(),
                        "created_at": opened.created_at,
                    });
                    object
                        .as_object_mut()
                        .expect("object")
                        .extend(body.as_object().expect("object").clone());
                    emit_json(&object);
                    if !acked.contains(&opened.id) {
                        pending.push(opened.id.clone());
                    }
                }
                for failed in &batch.failed {
                    eprintln!(
                        "flintmsg: отклонён envelope {}: {}",
                        failed.id, failed.error
                    );
                }
                if !pending.is_empty() {
                    match messenger.ack_signed(&identity, &pending) {
                        Ok(count) => {
                            // Stop retrying ids only after the gateway accepted
                            // the signed ACK; a failed ACK is retried next poll.
                            for id in pending {
                                acked.insert(id);
                            }
                            emit_json(&json!({"event": "acked", "count": count}));
                        }
                        Err(e) => eprintln!("flintmsg: ACK: {e} (повтор при следующем poll)"),
                    }
                }
            }
            Err(error) => eprintln!("flintmsg: fetch: {error}"),
        }
        std::thread::sleep(Duration::from_secs(poll_secs));
    }
}

fn receive_once(
    messenger: &Messenger<GatewayTransport>,
    identity: &Identity,
    limit: usize,
    ack: bool,
) -> CliResult<usize> {
    let batch = messenger
        .fetch_open(identity, limit)
        .map_err(|e| format!("fetch: {e}"))?;
    let mut ids: Vec<String> = batch.duplicates.clone();
    for opened in &batch.opened {
        let body = match &opened.kind {
            MessageKind::Text { text } => json!({"kind": "text", "text": text}),
            MessageKind::Slatepack { armor, encrypted } => {
                json!({"kind": "slatepack", "armor": armor, "encrypted": encrypted})
            }
        };
        let mut object = json!({
            "event": "message",
            "id": opened.id,
            "from": opened.sender.to_string(),
            "created_at": opened.created_at,
        });
        object
            .as_object_mut()
            .expect("object")
            .extend(body.as_object().expect("object").clone());
        emit_json(&object);
        ids.push(opened.id.clone());
    }
    for failed in &batch.failed {
        eprintln!(
            "flintmsg: отклонён envelope {}: {}",
            failed.id, failed.error
        );
    }
    if ack {
        let acked = messenger
            .ack_signed(identity, &ids)
            .map_err(|e| format!("ACK: {e}"))?;
        emit_json(&json!({"event": "acked", "count": acked}));
    }
    Ok(batch.opened.len())
}

// ---------- send ----------

fn cmd_send(args: &[String]) -> CliResult<()> {
    let mut identity_path = PathBuf::from("identity.json");
    let mut to: Option<String> = None;
    let mut text: Option<String> = None;
    let mut armor_file: Option<PathBuf> = None;
    let mut ttl_secs = DEFAULT_TTL_SECS;
    let mut relay: Option<String> = None;
    let mut plain = false;
    let mut data = PathBuf::from("flintmsg-data");
    let mut tor_state: Option<PathBuf> = None;
    let mut bridge_line: Option<String> = None;
    let mut bridge_bin: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--identity" | "-i" => identity_path = PathBuf::from(take(args, &mut i, "--identity")?),
            "--to" => to = Some(take(args, &mut i, "--to")?),
            "--text" => text = Some(take(args, &mut i, "--text")?),
            "--armor-file" => armor_file = Some(PathBuf::from(take(args, &mut i, "--armor-file")?)),
            "--ttl-secs" => {
                ttl_secs = take(args, &mut i, "--ttl-secs")?
                    .parse()
                    .map_err(|_| "--ttl-secs: ожидалось число".to_string())?
            }
            "--relay" => relay = Some(take(args, &mut i, "--relay")?),
            "--plain" => plain = true,
            "--data" => data = PathBuf::from(take(args, &mut i, "--data")?),
            "--tor-state" => tor_state = Some(PathBuf::from(take(args, &mut i, "--tor-state")?)),
            "--bridge-line" => bridge_line = Some(take(args, &mut i, "--bridge-line")?),
            "--bridge-bin" => bridge_bin = Some(PathBuf::from(take(args, &mut i, "--bridge-bin")?)),
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("send: неизвестный аргумент {other:?}")),
        }
        i += 1;
    }
    let (identity, _seed) = load_identity(&identity_path)?;
    let to_raw = to.ok_or_else(|| "send: обязателен --to <address>".to_string())?;
    let recipient: Address = to_raw
        .trim()
        .parse()
        .map_err(|e: flint_messenger::Error| format!("--to: {e}"))?;
    let kind = match (text, armor_file) {
        (Some(text), None) => MessageKind::Text { text },
        (None, Some(path)) => {
            let armor = fs::read_to_string(&path)
                .map_err(|e| format!("--armor-file {}: {e}", path.display()))?;
            MessageKind::Slatepack {
                armor: armor.trim().to_string(),
                encrypted: true,
            }
        }
        _ => return Err("send: нужен ровно один из --text или --armor-file".into()),
    };
    let tor = TorOptions {
        state_dir: tor_state.unwrap_or_else(|| data.join("arti-client-state")),
        bridge_line,
        bridge_bin,
    };
    let remote = match &relay {
        Some(url) => remote_url(plain, url)?,
        None => {
            if plain {
                return Err(
                    "direct-доставка на onion требует Arti; для relay-теста используйте --relay URL --plain"
                        .into(),
                );
            }
            recipient.onion_url()
        }
    };
    let connector = make_connector(plain, &tor)?;
    let transport = GatewayTransport::with_connector(&remote, connector)
        .map_err(|e| format!("transport: {e}"))?;
    let messenger = Messenger::new(transport);
    let envelope = messenger
        .send(&identity, &recipient, kind, ttl_secs)
        .map_err(|e| format!("send: {e}"))?;
    emit_json(&json!({
        "event": "sent",
        "id": envelope.id,
        "to": recipient.to_string(),
        "via": if relay.is_some() { "relay" } else { "direct" },
        "remote": remote,
    }));
    Ok(())
}

// ---------- fetch ----------

fn cmd_fetch(args: &[String]) -> CliResult<()> {
    let mut identity_path = PathBuf::from("identity.json");
    let mut source: Option<String> = None;
    let mut plain = false;
    let mut limit = 20usize;
    let mut ack = true;
    let mut tor_state: Option<PathBuf> = None;
    let mut bridge_line: Option<String> = None;
    let mut bridge_bin: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--identity" | "-i" => identity_path = PathBuf::from(take(args, &mut i, "--identity")?),
            "--source" | "--relay" => source = Some(take(args, &mut i, "--source")?),
            "--plain" => plain = true,
            "--limit" => {
                limit = take(args, &mut i, "--limit")?
                    .parse()
                    .map_err(|_| "--limit: ожидалось число".to_string())?
            }
            "--no-ack" => ack = false,
            "--tor-state" => tor_state = Some(PathBuf::from(take(args, &mut i, "--tor-state")?)),
            "--bridge-line" => bridge_line = Some(take(args, &mut i, "--bridge-line")?),
            "--bridge-bin" => bridge_bin = Some(PathBuf::from(take(args, &mut i, "--bridge-bin")?)),
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("fetch: неизвестный аргумент {other:?}")),
        }
        i += 1;
    }
    let (identity, _seed) = load_identity(&identity_path)?;
    let source = source.ok_or_else(|| "fetch: обязателен --source <url>".to_string())?;
    let remote = remote_url(plain, &source)?;
    let tor = TorOptions {
        state_dir: tor_state
            .unwrap_or_else(|| PathBuf::from("flintmsg-data").join("arti-client-state")),
        bridge_line,
        bridge_bin,
    };
    let connector = make_connector(plain, &tor)?;
    let transport = GatewayTransport::with_connector(&remote, connector)
        .map_err(|e| format!("transport: {e}"))?;
    let messenger = Messenger::new(transport);
    let opened = receive_once(&messenger, &identity, limit, ack)?;
    emit_json(&json!({"event": "fetched", "opened": opened, "source": remote}));
    Ok(())
}

// ---------- output ----------

fn emit_json(value: &serde_json::Value) {
    match serde_json::to_string(value) {
        Ok(line) => {
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            let _ = writeln!(lock, "{line}");
            let _ = lock.flush();
        }
        Err(error) => eprintln!("flintmsg: JSON: {error}"),
    }
}
