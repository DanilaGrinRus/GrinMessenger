# Releases and CI

## What CI checks

`.github/workflows/ci.yml` runs on every push to `main`, every pull request and
manually:

- **Windows + Linux matrix**: `cargo build --workspace --locked`,
  `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets
  -- -D warnings`; `cargo fmt --check` on Linux.
- The ignored **real relay process test** (`relay_live`) on Linux.
- **Dependency audit** (`rustsec/audit-check`).

The toolchain is pinned to **1.99.1** in CI so formatting/clippy results are
reproducible (`rust-version = "1.92"` in `Cargo.toml` is the minimum). Builds
use plain `cargo` against `Cargo.lock`; no vendor directory is needed on CI.

## What a release contains

Pushing a tag `v*` (or running `release.yml` manually for build-only artifacts)
produces two platform bundles:

| Platform | Artifact |
|---|---|
| Linux x86_64 | `flintmsg`, `flint-relay`, `flintmsg-ui`, `flintmsg-walletd`, `flintmsg-wallet-ui` |
| Windows x86_64 | the same binaries with `.exe` |
| Both | `SHA256SUMS-linux-x86_64.txt` / `SHA256SUMS-windows-x86_64.txt` (one per platform) |
| Both | `Cargo.lock.release` (exact dependency graph) and `sbom.cyclonedx.json` (CycloneDX SBOM generated with Syft/Anchore) |

The job attaches them to a GitHub Release with generated release notes. The
artifacts are **not code-signed**: verify the SHA-256 sums and build from the
tagged source if you need stronger provenance.

## Cutting a release

```powershell
git checkout main
git pull
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test -p flint-gateway --test relay_live --locked -- --ignored --test-threads=1
git tag -a vX.Y.Z -m "GrinMessenger vX.Y.Z"
git push origin vX.Y.Z
```

Then wait for the `release` workflow; confirm the GitHub Release contains both
platform bundles, `SHA256SUMS-*.txt` and the SBOM. Check the live site deploy
(`pages` workflow) if `site/**` changed.

## Verifying a download

```powershell
# Windows
Get-FileHash .\flintmsg-walletd.exe -Algorithm SHA256
Get-Content .\SHA256SUMS-windows-x86_64.txt

# Linux/macOS
sha256sum -c SHA256SUMS-linux-x86_64.txt
```

`Cargo.lock` is included so the exact dependency graph can be reproduced with
`cargo build --release --locked` from the tagged commit. The CycloneDX SBOM lists
the dependency inventory for vulnerability tooling.

## Versioning

- `0.x` — DEV/testnet; breaking protocol/API changes are allowed between minor
  versions and are documented in `docs/PROTOCOL.md` / `docs/SECURITY.md`.
- Envelope v1 and v2 are distinguished by the envelope `version` field; clients
  accept both, new messages use the current version.
- Wallet API is `/v1`; breaking changes require `/v2` and a capability bump in
  `GET /v1/capabilities`.
- The UI/console are local tools; their URLs are printed at startup and are not
  a stable public API.

## Rollback

Releases are immutable tags. To roll back an installation, download the previous
tag's bundle, verify `SHA256SUMS.txt`, and keep the existing data directory:
wallet/messenger storage is encrypted and versioned; identity files are
compatible across 0.x as long as the seed/network are unchanged. Never downgrade
by replacing a data directory — restore from backup instead.
