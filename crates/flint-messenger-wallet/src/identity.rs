// SPDX-License-Identifier: MIT OR Apache-2.0
//! Identity/seed handling for wallet integration.
//!
//! The wallet owns the seed. This module never writes a seed unless the caller
//! explicitly asks for a generated identity file (standalone/test use). Key
//! derivation is domain-separated:
//!
//! ```text
//! messaging_seed = HKDF-SHA256(
//!     salt = "flint-messenger/wallet-integration/v1",
//!     ikm  = wallet_master_seed,
//!     info = label (e.g. "grin-wallet/mainnet/account-0/messenger"),
//!     len  = 32)
//! storage_key    = HKDF-SHA256(
//!     salt = "flint-messenger/storage-key/v1",
//!     ikm  = messaging_seed,
//!     info = network hrp,
//!     len  = 32)
//! ```
//!
//! Using a dedicated seed means a compromise of the messenger data directory or
//! of the messenger code cannot sign wallet transactions; the wallet master seed
//! itself is never given to the daemon when the wallet derives the subkey.

use std::fs;
use std::path::Path;

use flint_messenger::{Identity, Result};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

pub const NETWORK_HRP_MAINNET: &str = "grin";
pub const NETWORK_HRP_TESTNET: &str = "tgrin";

/// Map a wallet network label to the address HRP.
pub fn network_hrp(network: &str) -> &'static str {
    match network {
        "grin" | "mainnet" => NETWORK_HRP_MAINNET,
        _ => NETWORK_HRP_TESTNET,
    }
}

/// Derive a dedicated 32-byte messaging seed from a wallet master seed.
///
/// `label` must be stable for a given wallet/account, for example
/// `"grin-wallet/mainnet/account-0/messenger"`; changing it changes the
/// messenger address. The wallet should store only `label`, not the derived
/// seed, and re-derive it at every start.
pub fn derive_messaging_seed(master_seed: &[u8], label: &str) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(b"flint-messenger/wallet-integration/v1"), master_seed);
    let mut out = [0u8; 32];
    hk.expand(label.as_bytes(), &mut out)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    out
}

/// Derive the symmetric key that encrypts local history/contacts/read markers.
/// It is independent from the envelope signing key.
pub fn storage_key(seed: &[u8; 32], network: &str) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(b"flint-messenger/storage-key/v1"), seed);
    let mut out = [0u8; 32];
    hk.expand(network_hrp(network).as_bytes(), &mut out)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    out
}

/// Build an identity from a raw 32-byte seed for the given network.
pub fn identity_from_seed(seed: [u8; 32], network: &str) -> Result<Identity> {
    Identity::from_seed(seed, network_hrp(network))
}

/// Read a seed from a file. Accepts one line of 64 hex chars (with optional
/// `0x`) or base64 of the 32 bytes. The file is expected to be `0600`; the
/// daemon never prints the seed.
pub fn read_seed_file(path: &Path) -> std::result::Result<Zeroizing<[u8; 32]>, String> {
    let text =
        fs::read_to_string(path).map_err(|e| format!("seed file {}: {e}", path.display()))?;
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .ok_or_else(|| format!("seed file {} is empty", path.display()))?;
    parse_seed(line).map_err(|e| format!("seed file {}: {e}", path.display()))
}

/// Parse a seed given as hex or base64.
pub fn parse_seed(raw: &str) -> std::result::Result<Zeroizing<[u8; 32]>, String> {
    let raw = raw.trim();
    if let Some(hex) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        return parse_hex(hex);
    }
    if raw.len() == 64 && raw.chars().all(|c| c.is_ascii_hexdigit()) {
        return parse_hex(raw);
    }
    let bytes = data_encoding::BASE64
        .decode(raw.as_bytes())
        .map_err(|e| format!("seed is neither 64-char hex nor base64: {e}"))?;
    let seed: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| "seed must be 32 bytes".to_string())?;
    Ok(Zeroizing::new(seed))
}

fn parse_hex(hex: &str) -> std::result::Result<Zeroizing<[u8; 32]>, String> {
    let bytes = data_encoding::HEXLOWER
        .decode(hex.trim().as_bytes())
        .map_err(|e| format!("seed hex: {e}"))?;
    let seed: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| "seed must be 32 bytes".to_string())?;
    Ok(Zeroizing::new(seed))
}

/// JSON identity file shared with the CLI, used for standalone runs and tests.
/// It contains a secret seed: never publish it, keep permissions `0600`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct IdentityFile {
    pub version: u8,
    pub network: String,
    pub seed_hex: String,
}

impl IdentityFile {
    pub fn from_seed(network: &str, seed: &[u8; 32]) -> Self {
        Self {
            version: 1,
            network: if network_hrp(network) == NETWORK_HRP_MAINNET {
                "grin".to_string()
            } else {
                "tgrin".to_string()
            },
            seed_hex: data_encoding::HEXLOWER.encode(seed),
        }
    }
}

/// Load (identity, seed, network) from a JSON identity file.
pub fn load_identity_file(
    path: &Path,
) -> std::result::Result<(Identity, Zeroizing<[u8; 32]>, String), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("identity {}: {e}", path.display()))?;
    let file: IdentityFile =
        serde_json::from_str(&text).map_err(|e| format!("identity JSON: {e}"))?;
    if file.version != 1 {
        return Err(format!(
            "identity version {}: only 1 is supported",
            file.version
        ));
    }
    let seed = parse_seed(&file.seed_hex).map_err(|e| format!("identity seed_hex: {e}"))?;
    let identity = identity_from_seed(*seed, &file.network)
        .map_err(|e| format!("identity {}: {e}", path.display()))?;
    Ok((identity, seed, file.network))
}

/// Create `path` with a fresh random seed if the file does not exist yet, and
/// return the loaded identity/seed. Existing files are loaded unchanged.
pub fn save_generated_identity(
    path: &Path,
    network: &str,
) -> std::result::Result<(Identity, Zeroizing<[u8; 32]>, String), String> {
    if path.exists() {
        return load_identity_file(path);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("identity dir: {e}"))?;
    }
    let mut seed = Zeroizing::new([0u8; 32]);
    getrandom::getrandom(seed.as_mut()).map_err(|e| format!("RNG: {e}"))?;
    let file = IdentityFile::from_seed(network, &seed);
    let text = serde_json::to_string_pretty(&file).map_err(|e| format!("identity JSON: {e}"))?;
    write_secret_file(path, format!("{text}\n").as_bytes())?;
    let identity = identity_from_seed(*seed, network).map_err(|e| format!("identity: {e}"))?;
    Ok((identity, seed, file.network))
}

/// Write a file that contains secret material and restrict it on Unix.
pub fn write_secret_file(path: &Path, data: &[u8]) -> std::result::Result<(), String> {
    fs::write(path, data).map_err(|e| format!("write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, perms).map_err(|e| format!("chmod {}: {e}", path.display()))?;
    }
    Ok(())
}

/// Generate a fresh random 32-byte API token as 64 lowercase hex characters.
pub fn generate_token() -> std::result::Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|e| format!("RNG: {e}"))?;
    Ok(data_encoding::HEXLOWER.encode(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivation_is_deterministic_and_domain_separated() {
        let master = [7u8; 64];
        let a = derive_messaging_seed(&master, "wallet/mainnet/account-0/messenger");
        let b = derive_messaging_seed(&master, "wallet/mainnet/account-0/messenger");
        let c = derive_messaging_seed(&master, "wallet/mainnet/account-1/messenger");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, master[..32]);
    }

    #[test]
    fn storage_key_depends_on_network_and_seed() {
        let seed = [3u8; 32];
        let testnet = storage_key(&seed, "tgrin");
        let mainnet = storage_key(&seed, "grin");
        assert_ne!(testnet, mainnet);
        assert_ne!(testnet, seed);
        assert_eq!(testnet, storage_key(&seed, "testnet"));
    }

    #[test]
    fn seed_parsing_accepts_hex_and_base64() {
        let seed = [9u8; 32];
        let hex = data_encoding::HEXLOWER.encode(&seed);
        assert_eq!(*parse_seed(&hex).unwrap(), seed);
        assert_eq!(*parse_seed(&format!("0x{hex}")).unwrap(), seed);
        let b64 = data_encoding::BASE64.encode(&seed);
        assert_eq!(*parse_seed(&b64).unwrap(), seed);
        assert!(parse_seed("not a seed").is_err());
    }

    #[test]
    fn generated_identity_is_stable_across_reload() {
        let dir = std::env::temp_dir().join(format!(
            "flint-wallet-identity-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("identity.json");
        let (first, seed, network) = save_generated_identity(&path, "tgrin").unwrap();
        let (second, seed2, network2) = save_generated_identity(&path, "tgrin").unwrap();
        assert_eq!(first.address(), second.address());
        assert_eq!(*seed, *seed2);
        assert_eq!(network, "tgrin");
        assert_eq!(network2, "tgrin");
        std::fs::remove_dir_all(dir).ok();
    }
}
