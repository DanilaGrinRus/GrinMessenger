# GrinMessageModule

Самостоятельный модуль мессенджера, выделенный из проекта
[Flint](https://github.com/DanilaGrinRus/Flint): подписанные и зашифрованные
envelope-ы, прямая доставка на Tor v3 onion через **Arti (in-process)** и
опциональный store-and-forward relay. Без кошелька, Grin-ноды, RPC и UI.

Репозиторий: <https://github.com/DanilaGrinRus/GrinMessageModule>. Крейты и
бинари сохраняют исторические имена `flint-*` / `flintmsg` / `flint-relay` как
provenance выделения из Flint messenger; это не часть кошелька Flint.

| | |
|---|---|
| Репозиторий | <https://github.com/DanilaGrinRus/GrinMessageModule> |
| Лицензия | MIT OR Apache-2.0 (см. `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`) |
| Источник | Flint `20efa26a953f83a36ec0fe248e390d0031d01a95` (2026-10-30) |
| Provenance | [PROVENANCE.md](PROVENANCE.md), [evidence/20261107-extraction](evidence/20261107-extraction/) |
| Протокол | [docs/PROTOCOL.md](docs/PROTOCOL.md) |
| Архитектура | [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| Сторонние компоненты | [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) |

Статус (2026-11-07): **извлечение и адаптация выполнены; offline-сборка PASS,
125 автотестов PASS (0 failed, 3 ignored), loopback e2e и реальные процессы CLI
PASS. Live Tor — FAILED/NOT VERIFIED: в этом окружении Arti не завершил bootstrap
за 300 с (сеть Tor недоступна); это не выдаётся за PASS.** Подробности —
[evidence/20261107-extraction/REPORT.md](evidence/20261107-extraction/REPORT.md).
Это DEV/testnet-инструмент, не аудированный и не готовый к mainnet.

## Крейты

| Крейт | Бинарь | Что делает |
|---|---|---|
| `flint-messenger` | — | envelope v1, crypto, PoW, ACK, store/history/contacts, транспорты (`GatewayTransport`, `DirectOnionTransport`, Nostr) |
| `flint-tor-arti` | — | in-process Tor: onion v3 из ed25519 seed, onion service, HTTP-клиент через Arti |
| `flint-gateway` | `flint-relay` | loopback gateway / standalone relay без кошелька |
| `flint-messenger-cli` | `flintmsg` | two-peer CLI: keygen/address/serve/send/fetch |

## Сборка

Нужен Rust MSVC (edition 2021, `rust-version = "1.92"`; проверено на 1.99).

```powershell
cargo build --workspace
cargo test --workspace
```

Этот workspace лежит внутри `C:\DHW`, где унаследованный
`C:\DHW\.cargo\config.toml` заменяет crates.io на offline-вендор
`C:\DHW\vendor` — поэтому сборка проходит без сети. На другой машине просто
`cargo build` подтянет зависимости из crates.io по `Cargo.lock`; для полностью
offline-сборки перенесите вендор и `.cargo/config.toml` рядом с проектом.

`profile.dev` использует `debug = 0` (MSVC choking на длинных путях зависимостей).
Release-сборка: `cargo build --release --workspace`.

## Быстрый старт: direct mode (Tor/Arti)

Нужна сеть Tor (Arti bootstrap; первый запуск может занять 30–150 с). Оба
участника должны быть online — direct-доставка не хранит сообщения вне
получателя. Для запуска без Tor см. relay mode ниже.

```powershell
# 1) идентичности (seed-файлы; НЕ публиковать)
cargo run -p flint-messenger-cli --bin flintmsg -- keygen --out alice.json
cargo run -p flint-messenger-cli --bin flintmsg -- keygen --out bob.json
cargo run -p flint-messenger-cli --bin flintmsg -- address --identity bob.json
# { "address": "tgrin1…", "onion": "….onion" }

# 2) Bob поднимает onion + loopback gateway и печатает входящие JSON-строки
cargo run -p flint-messenger-cli --bin flintmsg -- serve --identity bob.json --data bob-data

# 3) Alice отправляет на адрес Bob (onion выводится из адреса)
cargo run -p flint-messenger-cli --bin flintmsg -- send --identity alice.json `
  --to tgrin1… --text "hello over arti"
```

`serve` печатает `{"event":"ready",…}` и затем `{"event":"message",…}` для
каждого сообщения; после печати сообщение подтверждается signed ACK. Первый
Arti bootstrap клиента/сервиса может занять до 150 с (таймаут настраивается в
коде `flint-tor-arti`).

## Relay mode (store-and-forward)

```powershell
# relay (loopback для теста; на практике публикуйте через onion)
cargo run -p flint-gateway --bin flint-relay -- --listen 127.0.0.1:8080 --store relay-store

# отправка и приём через relay; --plain = TCP без Tor (только loopback/clearnet)
cargo run -p flint-messenger-cli --bin flintmsg -- send --identity alice.json `
  --to tgrin1… --text "hi via relay" --relay http://127.0.0.1:8080 --plain
cargo run -p flint-messenger-cli --bin flintmsg -- fetch --identity bob.json `
  --source http://127.0.0.1:8080 --plain
```

Без `--plain` CLI ходит через Arti и умеет `.onion`-relay. `flint-relay`
включает `require_pow` и `require_signed_ack`; сообщения — opaque envelope-ы,
relay не видит plaintext.

## Использование как библиотеки

```rust
use flint_messenger::{Address, Identity, MessageKind, Messenger, DEFAULT_TTL_SECS};
use flint_messenger::transport::GatewayTransport;

let identity = Identity::from_seed(seed, "tgrin")?;
let recipient: Address = "tgrin1…".parse()?;
let messenger = Messenger::new(GatewayTransport::new("http://127.0.0.1:8080")?);
let envelope = messenger.send(
    &identity,
    &recipient,
    MessageKind::Text { text: "hello".into() },
    DEFAULT_TTL_SECS,
)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Для доставки через Arti реализуйте
`flint_messenger::transport::HttpConnector` поверх
`flint_tor_arti::TorHttpClient` (ровно так делает `flintmsg`) и передайте его в
`GatewayTransport::with_connector` / `DirectOnionTransport::with_connector`.
Для внешнего Tor с SOCKS5 сохранены `GatewayTransport::with_socks` и
`DirectOnionTransport::new`.

## Проверки

```powershell
cargo test --workspace                     # unit + integration (без live Tor)
cargo test -p flint-tor-arti --test live_onion -- --ignored   # нужна сеть Tor
cargo test -p flint-gateway --test relay_live -- --ignored    # spawn flint-relay
```

Результаты прогонов (команды, PASS/FAIL, live Tor NOT VERIFIED при недоступной
сети) — в [evidence/20261107-extraction/](evidence/20261107-extraction/).
Сводка: `cargo build --workspace --offline --locked` — PASS;
`cargo test --workspace --offline --locked` — 125 passed / 0 failed / 3 ignored
(пишутся: `relay_live`, live-Nostr, `live_onion`); `cargo fmt --check` — PASS.

## Что не входит

- Кошелёк, Grin-нода, owner/foreign API, Slatepack-транзакции и их автоматическая
  обработка (S1/S2).
- Slint UI/desktop-приложение Flint, iOS-клиент.
- Группы, вложения, ratchet/forward secrecy, гарантированная offline-доставка,
  публичный abuse-control и собственный P2P-протокол.
- Nostr-транспорт присутствует в библиотеке как отдельный вариант, но CLI v1 его
  не включает.

## Ограничения и честные утверждения

- Протокол v1 не даёт forward secrecy и защиты после компрометации ключа.
- Relay/direct видят метаданные (адрес получателя, размер, время); relay не
  читает payload, но может удалять/дублировать сообщения.
- Tor скрывает сетевой маршрут, но не является заменой end-to-end шифрования —
  оно обеспечивается envelope-ом.
- Проект не заявляет анонимность, отсутствие ошибок или готовность к mainnet.
- CLI не ведёт постоянную историю/контакты: это демонстрационный инструмент;
  библиотечные `History`, `ContactBook`, `FileMailbox` доступны для встраивания.
