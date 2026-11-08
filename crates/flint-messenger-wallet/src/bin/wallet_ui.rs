// SPDX-License-Identifier: MIT OR Apache-2.0
//! `flintmsg-wallet-ui` — loopback web console for the wallet sidecar API.
//!
//! The page talks to this process (same origin, no CORS). This process proxies
//! `/api/v1/*` to `flintmsg-walletd` and injects the bearer token server-side, so
//! the token never reaches browser JavaScript and the wallet daemon's API does
//! not need CORS headers. Both listeners are forced to loopback.

use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use flint_messenger_wallet::server::bind;
use serde_json::{json, Value};

const PAGE: &str = include_str!("wallet_ui.html");
const MAX_HEADER: usize = 64 * 1024;
const MAX_BODY: usize = 1024 * 1024;
const UI_VERSION: u8 = 1;

struct Options {
    api: String,
    token_file: PathBuf,
    token_env: Option<String>,
    listen: String,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("flintmsg-wallet-ui: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") || args.is_empty() {
        println!("{USAGE}");
        return Ok(());
    }
    let options = parse_args(&args)?;
    let (api_host, api_port) = parse_api(&options.api)?;
    let token = load_token(&options.token_file, options.token_env.as_deref())?;
    let (listener, addr) = bind(&options.listen)?;
    println!(
        "{}",
        serde_json::to_string(&json!({
            "event": "ready",
            "url": format!("http://{addr}/"),
            "api": options.api,
            "wallet_ui_version": UI_VERSION,
            "auth": "proxy-injected bearer token (never exposed to the browser)",
        }))
        .map_err(|e| e.to_string())?
    );
    std::io::stdout().flush().ok();
    serve(listener, api_host, api_port, token)?;
    Ok(())
}

const USAGE: &str = "\
flintmsg-wallet-ui — loopback web console for flintmsg-walletd

USAGE:
  flintmsg-wallet-ui --api http://127.0.0.1:<port> --token-file <file>
                     [--listen 127.0.0.1:0]

The console serves its page and proxies /api/v1/* to the wallet daemon,
injecting the bearer token server-side. The token is never sent to the browser
and is not printed. Both listeners must be loopback.
";

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut api = None;
    let mut token_file = None;
    let mut token_env = None;
    let mut listen = "127.0.0.1:0".to_string();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--api" => api = Some(take(args, &mut index, "--api")?),
            "--token-file" => {
                token_file = Some(PathBuf::from(take(args, &mut index, "--token-file")?))
            }
            "--token-env" => token_env = Some(take(args, &mut index, "--token-env")?),
            "--listen" => listen = take(args, &mut index, "--listen")?,
            other => return Err(format!("unknown argument {other:?}")),
        }
        index += 1;
    }
    let api = api.ok_or_else(|| "--api is required (e.g. http://127.0.0.1:50642)".to_string())?;
    let token_file = match (token_file, token_env.is_some()) {
        (Some(path), _) => path,
        (None, true) => PathBuf::from("<env>"),
        (None, false) => {
            return Err("--token-file or --token-env is required".into());
        }
    };
    Ok(Options {
        api,
        token_file,
        token_env,
        listen,
    })
}

fn take(args: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| format!("{flag}: missing value"))
}

fn parse_api(url: &str) -> Result<(String, u16), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("--api {url:?}: only http:// loopback URLs are supported"))?;
    let rest = rest.trim_end_matches('/');
    if rest.contains('/') {
        return Err(format!("--api {url:?}: base URL must not contain a path"));
    }
    let (host, port) = match rest.rsplit_once(':') {
        Some((host, port)) => (
            host.to_string(),
            port.parse::<u16>()
                .map_err(|_| format!("--api {url:?}: bad port"))?,
        ),
        None => (rest.to_string(), 80),
    };
    if host.is_empty() {
        return Err(format!("--api {url:?}: missing host"));
    }
    let ip: IpAddr = host
        .parse()
        .map_err(|_| format!("--api {url:?}: host must be a literal loopback IP"))?;
    if !ip.is_loopback() {
        return Err(format!(
            "--api {url:?}: refusing a non-loopback wallet daemon address"
        ));
    }
    Ok((host, port))
}

fn load_token(path: &PathBuf, env_name: Option<&str>) -> Result<String, String> {
    let token = if let Some(name) = env_name {
        std::env::var(name).map_err(|_| format!("environment variable {name} is not set"))?
    } else {
        fs::read_to_string(path).map_err(|e| format!("token file {}: {e}", path.display()))?
    };
    let token = token.trim().to_string();
    if token.len() < 32 {
        return Err(format!(
            "wallet daemon token is too short ({} chars); expected at least 32",
            token.len()
        ));
    }
    Ok(token)
}

struct Shared {
    api_host: String,
    api_port: u16,
    token: String,
}

fn serve(
    listener: TcpListener,
    api_host: String,
    api_port: u16,
    token: String,
) -> Result<(), String> {
    let addr = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {e}"))?;
    let shared = Arc::new(Shared {
        api_host,
        api_port,
        token,
    });
    let hosts = Arc::new(allowed_hosts(addr));
    for incoming in listener.incoming() {
        let shared = shared.clone();
        let hosts = hosts.clone();
        match incoming {
            Ok(stream) => {
                std::thread::spawn(move || handle_connection(stream, shared, hosts));
            }
            Err(error) => return Err(format!("accept: {error}")),
        }
    }
    Ok(())
}

/// `Host` values accepted for a listener bound to `addr` (exact port, loopback
/// only). DNS rebinding fails here: the browser sends the attacker's hostname.
fn allowed_hosts(addr: SocketAddr) -> Vec<String> {
    let port = addr.port();
    let mut hosts = vec![format!("localhost:{port}")];
    match addr.ip() {
        IpAddr::V4(ip) => hosts.push(format!("{ip}:{port}")),
        IpAddr::V6(ip) => hosts.push(format!("[{ip}]:{port}")),
    }
    hosts
}

fn guard(request: &Request, hosts: &[String]) -> Option<(u16, String)> {
    let host_ok = request
        .header("host")
        .map(|host| {
            hosts
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(host))
        })
        .unwrap_or(false);
    if !host_ok {
        return Some((403, "host not allowed".into()));
    }
    if let Some(origin) = request.header("origin") {
        let origin_ok = hosts
            .iter()
            .any(|allowed| origin.eq_ignore_ascii_case(&format!("http://{allowed}")));
        if !origin_ok {
            return Some((403, "origin not allowed".into()));
        }
    }
    if request.method == "POST" {
        let json = request
            .header("content-type")
            .and_then(|value| value.split(';').next())
            .map(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
            .unwrap_or(false);
        if !json {
            return Some((415, "content-type must be application/json".into()));
        }
    }
    None
}

fn handle_connection(mut stream: TcpStream, shared: Arc<Shared>, hosts: Arc<Vec<String>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(15)));
    let response = match read_request(&mut stream) {
        Ok(request) => match guard(&request, &hosts) {
            Some((status, error)) => Response::json(status, json!({"error": error})),
            None => route(&shared, &request),
        },
        Err(error) => Response::json(400, json!({"error": error})),
    };
    let _ = write_response(&mut stream, &response);
}

struct Request {
    method: String,
    path: String,
    query: String,
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

fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end;
    loop {
        let read = stream.read(&mut chunk).map_err(|e| format!("read: {e}"))?;
        if read == 0 {
            return Err("empty request".into());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > MAX_HEADER {
            return Err("request headers are too large".into());
        }
        if let Some(position) = find_bytes(&buffer, b"\r\n\r\n") {
            header_end = position + 4;
            break;
        }
    }
    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_ascii_uppercase();
    let target = parts.next().unwrap_or_default();
    if method.is_empty() || target.is_empty() {
        return Err("malformed request line".into());
    }
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_string(), query.to_string()),
        None => (target.to_string(), String::new()),
    };
    let mut length = 0usize;
    let mut headers = Vec::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                length = value
                    .parse::<usize>()
                    .map_err(|_| "bad Content-Length".to_string())?;
            }
            headers.push((name, value));
        }
    }
    if length > MAX_BODY {
        return Err("request body is too large".into());
    }
    let mut body = buffer[header_end..].to_vec();
    while body.len() < length {
        let read = stream
            .read(&mut chunk)
            .map_err(|e| format!("read body: {e}"))?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
        if body.len() > MAX_BODY {
            return Err("request body is too large".into());
        }
    }
    body.truncate(length);
    Ok(Request {
        method,
        path,
        query,
        headers,
        body,
    })
}

enum Response {
    Html(Vec<u8>),
    Json(u16, Vec<u8>),
}

impl Response {
    fn json(status: u16, value: Value) -> Self {
        Self::Json(
            status,
            serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec()),
        )
    }
}

fn route(shared: &Shared, request: &Request) -> Response {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") | ("GET", "/index.html") => Response::Html(PAGE.as_bytes().to_vec()),
        ("GET", "/api/meta") => Response::json(
            200,
            json!({
                "wallet_ui_version": UI_VERSION,
                "api": format!("http://{}:{}", shared.api_host, shared.api_port),
                "auth": "bearer injected server-side; not visible to the browser",
                "loopback_only": true,
            }),
        ),
        _ if request.path == "/api/v1" || request.path.starts_with("/api/v1/") => {
            let upstream_path = &request.path["/api".len()..];
            let upstream = if request.query.is_empty() {
                upstream_path.to_string()
            } else {
                format!("{upstream_path}?{}", request.query)
            };
            match forward(shared, &request.method, &upstream, &request.body) {
                Ok((status, body)) => Response::Json(status, body),
                Err(error) => Response::json(
                    502,
                    json!({"error": format!("wallet daemon unavailable: {error}")}),
                ),
            }
        }
        _ => Response::json(404, json!({"error": "not found"})),
    }
}

fn forward(
    shared: &Shared,
    method: &str,
    path: &str,
    body: &[u8],
) -> Result<(u16, Vec<u8>), String> {
    let address = format!("{}:{}", shared.api_host, shared.api_port);
    let mut stream = TcpStream::connect(&address).map_err(|e| format!("connect {address}: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(35)))
        .map_err(|e| e.to_string())?;
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        shared.token,
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .map_err(|e| e.to_string())?;
    stream.write_all(body).map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| format!("read response: {e}"))?;
    let text = String::from_utf8_lossy(&raw);
    let (header, payload) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "malformed wallet daemon response".to_string())?;
    let status: u16 = header
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| "malformed wallet daemon status line".to_string())?;
    Ok((status, payload.as_bytes().to_vec()))
}

fn write_response(stream: &mut TcpStream, response: &Response) -> std::io::Result<()> {
    let (status, content_type, payload) = match response {
        Response::Html(body) => (200u16, "text/html; charset=utf-8", body.as_slice()),
        Response::Json(status, body) => (*status, "application/json", body.as_slice()),
    };
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        502 => "Bad Gateway",
        500 => "Internal Server Error",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\
         Content-Security-Policy: default-src 'self'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src 'self' data:; connect-src 'self'\r\n\
         X-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n\r\n",
        payload.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(payload)?;
    stream.flush()
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_url_must_be_loopback_http() {
        assert_eq!(
            parse_api("http://127.0.0.1:1234").unwrap(),
            ("127.0.0.1".to_string(), 1234)
        );
        assert!(parse_api("https://127.0.0.1:1234").is_err());
        assert!(parse_api("http://10.0.0.5:1234").is_err());
        assert!(parse_api("http://127.0.0.1:1234/path").is_err());
    }

    #[test]
    fn request_framing_is_parsed() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream).unwrap();
            (request.method, request.path, request.query, request.body)
        });
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        client
            .write_all(
                b"POST /api/v1/send?x=1 HTTP/1.1\r\nHost: x\r\nContent-Length: 4\r\n\r\nhi!\n",
            )
            .unwrap();
        let (method, path, query, body) = thread.join().unwrap();
        assert_eq!(method, "POST");
        assert_eq!(path, "/api/v1/send");
        assert_eq!(query, "x=1");
        assert_eq!(body, b"hi!\n");
    }
}
