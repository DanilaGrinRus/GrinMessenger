# Архитектура GrinMessageModule

## Крейты

```text
flintmsg (CLI)
  ├── flint-messenger     envelope, crypto, PoW, ACK, store/history/contacts, transport
  │      └── (HttpConnector trait)  ──┐
  ├── flint-tor-arti      Arti client + onion service + onion-адреса
  │                                   │ CLI реализует Arti-connector поверх TorHttpClient
  └── flint-gateway       loopback gateway / standalone relay (flint-relay)
```

Зависимости однонаправленные:

- `flint-messenger` — без кошелька/ноды/UI; generic-крейты + Nostr-зависимости.
- `flint-tor-arti` — arti 0.43/tor-* 0.43, без `flint-messenger` и без grin.
- `flint-gateway` — relay/gateway: сам биндит loopback TCP-listener и хранит
  envelope-ы; без Tor/кошелька и без внешних HTTP-клиентов.
- `flint-messenger-cli` — связывает все три и реализует `HttpConnector` через
  `flint-tor-arti::TorHttpClient` (файл `src/main.rs`).

## Direct-режим (основной)

```text
Alice flintmsg send                      Bob flintmsg serve
  Identity(seed→address/onion)            Gateway loopback 127.0.0.1:P
  envelope v1 (seal+mine_pow+sign)  ──►   OnionService::start(seed, P)
  Arti TorHttpClient POST                 onion:80 → 127.0.0.1:P
  http://<bob.onion>/flint/v1/msg         poll loop: GET /flint/v1/msg
                                          envelope open → print → signed ACK
```

- Onion — единственный публичный endpoint; gateway слушает loopback.
- Payload доставляется в mailbox получателя; sender не нужен адрес получателя
  в сети-каталоге — onion выводится из адреса.
- Direct требует online-получателя; TTL ограничивает хранение.
- Сетевой слой — in-process Arti: `tor.exe`, `torrc`, Tor Expert Bundle и
  внешний SOCKS не обязательны.

## Relay-режим

```text
Alice flintmsg send --relay http://<relay>/  ──►  flint-relay (Gateway)
Bob   flintmsg fetch --relay http://<relay>/  ◄──  mailbox файлов/TTL/квоты
```

- `flint-relay` — `flint-gateway` c `require_signed_ack = true`,
  `require_pow = true`; сообщения — opaque envelope-ы.
- Relay может быть опубликован onion-сервисом (через Arti или внешний Tor) или
  запущен на loopback для тестов (`--plain` в CLI).
- ACK подписывается ключом получателя (`AckProof`), чужое сообщение удалить
  нельзя.

## CLI (`flintmsg`)

| Команда | Назначение |
|---|---|
| `keygen` | создать identity-файл (seed 32 байта, network) и напечатать address/onion |
| `address` | показать address и onion по identity-файлу |
| `serve` | gateway + onion (Arti), печать входящих JSON-строк, signed ACK |
| `send` | envelope → direct onion получателя через Arti, либо `--relay URL` (Arti или `--plain` loopback) |
| `fetch` | разово забрать/открыть/подтвердить сообщения из relay или loopback gateway |

CLI намеренно без постоянной истории/контактов: это демонстрационный и
диагностический инструмент; библиотечные `History`/`ContactBook`/`FileMailbox`
доступны для встраивания.

## Границы доверия

- **Envelope**: конфиденциальность и целостность даёт ChaCha20-Poly1305 +
  ed25519-подпись; relay не доверенный, но и не нужен для аутентификации
  (получатель проверяет подпись после расшифровки).
- **Relay**: видит `to`, размер, время, может удалять/дублировать; не видит
  payload/отправителя; не может подделать подпись.
- **Tor/Arti**: транспортная конфиденциальность; приложение не полагается на
  Tor для end-to-end шифрования (оно есть в envelope).
- **CLI**: identity-файл — секрет; stdout может содержать plaintext сообщений —
  это вывод для пользователя, не логи сервиса.

## Потоки и ресурсы

- Arti-рантайм — один на процесс (Tokio); onion service и TorHttpClient
  используют общий кэшированный клиент при одинаковом `TorConfig`.
- Onion service живёт, пока жив `OnionService` (Drop останавливает).
- CLI `serve` — блокирующий цикл poll (по умолчанию 2 с). Неподтверждённые
  envelope-ы хранятся в file-backed mailbox loopback-gateway (`<data>/mailbox`);
  CLI не ведёт отдельную расшифрованную историю.
- Ограничения relay: TTL/квоты/размеры — в `GatewayConfig`; CLI `serve`
  использует более строгие P2P-лимиты (32 КиБ payload, 30 на получателя,
  12 ч TTL), `flint-relay` — дефолты relay.

## Что не входит

- Кошелёк, Grin-нода, RPC/owner API, Slatepack-транзакции и их автоматическая
  обработка.
- Slint UI и десктопный lifecycle Flint.
- Группы, вложения, ratchet/forward secrecy, offline-гарантии, публичный
  abuse-control.
- Собственный P2P-протокол/сеть.

## Сборка и доказательства

- Offline-сборка использует вендор `C:\DHW\vendor` через унаследованный
  `C:\DHW\.cargo\config.toml`; на другой машине — обычный `cargo build`
  (crates.io) с `Cargo.lock`.
- Единожды зафиксированные прогоны: `evidence/20261107-extraction/`
  (сборка, тесты, loopback e2e, live Tor — PASS/FAIL/NOT VERIFIED).
