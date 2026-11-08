# Third-party notices

GrinMessenger is licensed under **MIT OR Apache-2.0** (see
[LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)). This file lists
the third-party components whose code or data is redistributed or required by
the project. `Cargo.lock` is the authoritative version inventory; this text is a
summary, not a replacement for upstream license texts.

## Grin wallet — Apache-2.0

`crates/flint-tor-arti` contains code adapted from `grin-wallet`
(`impls/src/tor/arti.rs`, `impls/src/tor/config.rs`):

- Upstream: <https://github.com/mimblewimble/grin-wallet>
- Pinned base used by Flint: `v5.5.1` / commit
  `11046cd197f8296e6cd9313502a9f2cbce6e262a`, with the Flint fork patch
  `third_party/grin-wallet-flint.patch`.
- License: Apache License 2.0. The adapted files retain the Apache-2.0
  attribution in their headers; the Grin wallet `LICENSE` text is the upstream
  Apache-2.0 text.
- The Flint copyright holder relicensed only the Flint-authored portions;
  Apache-2.0 portions remain Apache-2.0 and are redistributed here under
  Apache-2.0 (compatible with the project's `MIT OR Apache-2.0` terms).

## Arti / Tor crates — MIT OR Apache-2.0

The in-process Tor layer uses the official Rust Tor implementation, pinned at
Arti 0.43.x (the same version the upstream Flint workspace used):

`arti-client 0.43.0`, `tor-rtcompat`, `tor-hsservice`, `tor-hsrproxy`,
`tor-keymgr`, `tor-llcrypto`, `tor-hscrypto` and their transitive `tor-*`
dependencies, all `0.43.0`.

- Upstream: <https://gitlab.torproject.org/tpo/core/arti>
- License: MIT OR Apache-2.0.
- These crates implement the Tor protocol; the project does not bundle the Tor
  network, directory authorities, or the `tor.exe` / Tor Expert Bundle binaries.

## Rust crate dependencies

All crates in `Cargo.lock` are used under their respective licenses (mostly
MIT / Apache-2.0 / BSD / ISC). Notable direct dependencies:

- `ring 0.17` — ISC-style / Apache-2.0 / MIT (legacy keywrap PBKDF2).
- `ed25519-dalek 2`, `curve25519-dalek 4`, `x25519-dalek 2`,
  `chacha20poly1305 0.9`, `argon2 0.5`, `sha2`, `sha3`, `hkdf` — MIT OR
  Apache-2.0.
- `secp256k1 0.33` — MIT (Nostr transport).
- `tungstenite 0.30`, `rustls 0.23` — MIT / Apache-2.0 (Nostr WebSocket
  transport).
- `hyper 1`, `hyper-util`, `http-body-util`, `bytes` — MIT (Arti HTTP client
  plumbing).
- `serde`, `serde_json`, `uuid`, `thiserror`, `getrandom`, `zeroize`, `bech32`,
  `data-encoding`, `base64` — MIT OR Apache-2.0.

The full license inventory can be generated from `Cargo.lock` with `cargo
license` / `cargo deny` on a machine with registry access; the vendored offline
build used for the evidence run does not include a license scanner. Before a
public release, run `cargo deny`/`cargo about`. Note that some transitive crates
are dual/ tri-licensed (for example `priority-queue` is `LGPL-3.0-or-later OR
MPL-2.0`, `r-efi` is `MIT OR Apache-2.0 OR LGPL-2.1-or-later`); the MIT/Apache/MPL
options are the selectable ones, and no GPL-only dependency is used.

## Landing page and web UI

The landing page under `site/` is adapted from the Flint project's own landing
page (`site/en/index.html`, `site/styles.css`, `site/assets/logo.svg`) with new
content and a new logo for GrinMessenger. Flint-authored material was relicensed
by the copyright holder under MIT OR Apache-2.0 (see `PROVENANCE.md`). No
external fonts, CDNs or third-party scripts are used; the page is fully
self-contained.

The screenshot `docs/assets/ui.png` / `site/assets/ui.png` is a capture of this
project's own web UI (demo run on loopback) and contains no third-party assets.
The UI itself is served by `crates/flint-messenger-ui`: a Rust std HTTP server
with one embedded HTML page, no WebView/browser engine or Node.js runtime added
to the project.

## Wallet integration layer

`crates/flint-messenger-wallet` is new project code (no copied wallet code). It
links only the existing workspace crates (`flint-messenger`, `flint-gateway`,
`flint-tor-arti`) plus crates that are already in `Cargo.lock` (serde, hkdf,
sha2, chacha20poly1305, zeroize, data-encoding, getrandom). No wallet RPC client,
Grin node code, Slatepack implementation or third-party file from Flint's wallet
is included; Slatepack armor is treated as an opaque payload supplied by the
wallet.

## Tor Project

"Tor" and the onion service address format are described by the Tor Project:
<https://spec.torproject.org/>. This project implements a Tor client and onion
service through Arti; it is not affiliated with or endorsed by the Tor Project.

## Flint upstream

Extracted Flint code and documentation: <https://github.com/DanilaGrinRus/Flint>
(commit `20efa26a953f83a36ec0fe248e390d0031d01a95`). See
[PROVENANCE.md](PROVENANCE.md).
