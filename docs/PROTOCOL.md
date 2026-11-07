# GrinMessageModule — протокол мессенджера v1 (standalone)

Статус: **извлечённая и адаптированная реализация Flint messenger v1**; проверка
сборки/тестов/локального e2e — в `evidence/20261107-extraction/`. Это не
утверждённый стандарт и не часть консенсуса Grin. Платёжная интеграция, кошелёк,
автоматическая обработка Slatepack (S1/S2), группы, вложения, ratchet и публичный
relay-сервис в этот проект не входят.

Проект содержит четыре крейта:

- `flint-messenger` — протокол: адреса, envelope, криптография, PoW, ACK,
  хранилище/история/контакты, транспорты (`GatewayTransport`,
  `DirectOnionTransport`, Nostr);
- `flint-tor-arti` — in-process Tor: onion v3 из того же ключа и HTTP-клиент
  через Arti;
- `flint-gateway` — standalone store-and-forward relay без кошелька и бинарь
  `flint-relay`;
- `flint-messenger-cli` — бинарь `flintmsg` (keygen/address/serve/send/fetch).

Wire-формат envelope v1, домены подписи/AAD/PoW и правила доставки сохранены от
Flint messenger v1; допускается адаптация форматов хранения и CLI-обвязки
(решение владельца).

## 1. Идентичность и адрес

Идентичность — 32-байтовый ed25519 seed. Долгосрочный ключ один:

1. `SigningKey::from_bytes(seed)` → ed25519 public key;
2. адрес — bech32(`hrp`, public key), `hrp` = `tgrin` (testnet) или `grin`
   (mainnet), т.е. формат slatepack-адреса;
3. Tor v3 onion выводится из того же ключа:
   `base32(PUBKEY || SHA3-256(".onion checksum" || PUBKEY || 0x03)[0..2] || 0x03)`;
   отдельный onion-адрес не нужен.

CLI хранит seed в файле-идентичности:

```json
{"version": 1, "network": "tgrin", "seed_hex": "<64 hex-символа>"}
```

Файл — секрет: не публиковать, не копировать в отчёты. Восстановление истории и
смена устройства — ответственность пользователя (в v1 нет отдельного
key-management/восстановления чата; потеря seed = потеря идентичности).

## 2. Envelope

JSON, бинарные поля — base64:

| Поле | Тип / смысл |
|---|---|
| `version` | u8, сейчас 1 |
| `id` | UUID, идентификатор доставки и ACK |
| `recipient` | адрес получателя (bech32) |
| `created_at` | Unix seconds |
| `ttl` | срок жизни в секундах |
| `ephemeral_key` | 32-байтовый X25519 public отправителя |
| `nonce` | 12 байт ChaCha20-Poly1305 |
| `ciphertext` | зашифрованный payload |
| `pow_nonce` / `pow_bits` | Proof-of-work; 0 у legacy-конвертов |
| `signature` | 64-байтовая ed25519 подпись отправителя |

Payload внутри ciphertext: `sender`, `type` = `text` | `slatepack`,
`text` либо `armor`, для slatepack `encrypted=true`, `created_at`.
Plaintext slatepack (`encrypted=false`) запрещён как при `seal`, так и при
`open`. Отправитель и тип не передаются открытыми полями, но это не полное
сокрытие отправителя (см. §9).

## 3. Криптография

- Ed25519 seed → `SHA-512(seed)[..32]` → X25519 scalar; public key — через
  Edwards→Montgomery.
- Отправитель создаёт ephemeral X25519, делает DH с public получателя.
- `HKDF-SHA256(salt = recipient X25519 public, info = "flint-messenger/v1/encryption")`
  даёт 32-байтовый ключ.
- ChaCha20-Poly1305; AAD связывает `version`, `id`, `recipient`, `created_at`,
  `ttl` с length-prefix строками.
- Domain-separated ed25519 transcript связывает эти поля, `ephemeral_key`,
  `nonce`, длину и ciphertext; для PoW-конверта также `pow_nonce`/`pow_bits`.
- SHA-256 PoW связывает `id`/`recipient`/`created_at`/`ttl`, commit ciphertext
  и nonce.
- Получатель после расшифровки видит sender и проверяет подпись его адресным
  ключом. Relay не может подделать подпись, но может проверить известного ему
  кандидата: подпись и transcript публичны (metadata-утечка v1; fix потребовал
  бы нового протокола).
- Никакой самодельной криптографии: используются готовые библиотеки
  (`ed25519-dalek`, `x25519-dalek`, `chacha20poly1305`, `hkdf`, `sha2`, `sha3`,
  `argon2`, `ring`).

## 4. Proof-of-work

`Envelope::seal` майнит PoW сразу:

- `POW_BITS` = 20 (release) / 12 (debug), `POW_MIN_BITS` = 18 (release) / 12
  (debug), максимум 24;
- `pow_bits=0` — legacy-конверт, публичный gateway с `require_pow` его
  отклоняет; `open` продолжает читать legacy для совместимости;
- relay/gateway с `require_pow` проверяет сложность до записи, получатель — до
  расшифровки.

PoW — только ограничение спама, не доказательство личности.

## 5. Доставка, TTL и gateway API

Доставка at-least-once: повтор присылает тот же подписанный envelope с тем же
`id`. TTL проверяется и mailbox, и получателем; `DEFAULT_TTL_SECS = 93600`
(26 ч), `MAX_TTL_SECS = 604800` (7 дней). Конкретный gateway может иметь более
строгий лимит и усекает TTL сохранённого сообщения.

| API | Тело / результат |
|---|---|
| `POST /flint/v1/msg` | `{id,to,payload,ttl_secs}` → `{id,expires_at}` |
| `GET /flint/v1/msg?to=address&limit=N` | сообщения `id/to/payload/created_at/expires_at`, без `from` |
| `POST /flint/v1/msg/<uuid>/ack` | `{to,id,ts,signature}`; подпись получателя, иначе 401/404 |
| `GET /flint/v1/health` | `{ok:true}` |

Правила:

- `id` — UUID, traversal запрещён;
- ACK transcript связывает `to/id/ts`; окно времени ±300 с; знание адреса не
  даёт права удалить чужое сообщение;
- relay/onion-listener требуют signed ACK; поддерживается «legacy» ACK без
  подписи только для локальных тестов (в `GatewayConfig.require_signed_ack =
  false`);
- дефолтные лимиты relay: до 100 сообщений на получателя, payload ≤ 64 КиБ,
  10000 всего; `flint-relay` дополнительно включает `require_pow` и
  `require_signed_ack`;
- ответ ограничивается до чтения/аллокации; квоты учитывают сохранённый store
  после restart; переполнение одного recipient не расходует квоту другого;
  ошибка storage не выдаёт ложный успех;
- логи не содержат текстов сообщений и секретов;
- серверу и клиенту нужны согласованные часы (для ACK).

## 6. Транспорты

### 6.1 Direct onion через Arti (основной)

- Получатель (`flintmsg serve`) поднимает loopback gateway и публикует onion
  через in-process Arti (без `tor.exe`); gateway — единственная цель onion.
- Отправитель строит envelope и делает `POST /flint/v1/msg` на
  `http://<onion-получателя>.onion` через Arti HTTP-клиент.
- Получатель забирает сообщения из своего loopback gateway, открывает,
  печатает пользователю и подписывает ACK.
- Direct требует, чтобы получатель был online: если его onion/gateway
  недоступен, сообщение не доставляется. Очередь/повторы у отправителя в CLI не
  хранятся.

### 6.2 Relay (store-and-forward)

- `flint-relay` — standalone relay без кошелька. Обе стороны могут обращаться к
  нему через Arti (`.onion`) или локально (`--plain` для тестов).
- Relay видит только opaque payload, адрес получателя, размер и время. Он не
  может прочитать содержимое или подделать подпись.
- Offline-получатель забирает сообщения при следующем запуске (в пределах TTL).

### 6.3 Совместимые варианты в библиотеке

- `GatewayTransport::new(url)` — plain HTTP (loopback/тесты).
- `GatewayTransport::with_socks(url, socks)` и
  `DirectOnionTransport::new(local, socks)` — доставка через внешний Tor SOCKS5.
- `HttpConnector`/`TcpConnector` — точка расширения: CLI реализует
  Arti-connector через `flint-tor-arti`, поэтому тот же протокол gateway
  работает поверх in-process Tor без изменения формата.
- Nostr-транспорт (NIP-44/NIP-59) присутствует в `flint-messenger` как
  отдельный вариант доставки, но в CLI v1 не включён и по умолчанию не
  используется; тихого fallback между транспортами нет.

## 7. Локальное хранилище (библиотека)

Извлечённые модули сохраняют прежнюю семантику, но CLI их не использует (v1 CLI
печатает сообщения в stdout; постоянная история не ведётся):

- `FileMailbox` — только подписанные encrypted envelopes, атомарная запись,
  TTL pruning, квоты, signed ACK;
- `History` — ChaCha20-Poly1305 под 32-байтовым профильным ключом, AAD =
  `peer+id`, nonce на запись;
- `ContactBook` — адреса/имена под history-key, атомарная запись, до 500
  контактов, имя ≤ 64 символов;
- history-key: на network/profile, keywrap v2 — Argon2id (64 МиБ, t=3, p=4) +
  ChaCha20-Poly1305; v1 PBKDF2-HMAC-SHA256 (100000) читается и переупаковывается
  в v2; кто владеет seed/паролем, тот владеет историей.

## 8. Транспортный обмен и ACK

- Получатель сначала durable-фиксирует принятое, затем подписывает ACK; ошибка
  ACK не скрывает уже открытые сообщения.
- Повторный/повреждённый envelope получает локальный quarantine/tombstone, а не
  ломает всю пачку; блокированный sender не попадает в историю.
- ACK — подтверждение доставки, а не подтверждение платежа: в standalone-проекте
  денежных операций нет.

## 9. Безопасность и пределы

- Forward secrecy нет: долгосрочный recipient key позволяет расшифровать старый
  трафик при его компрометации. Группы, вложения, ratchet, Sealed Sender, push и
  distributed abuse control — вне v1.
- Relay может задерживать/удалять/дублировать сообщения и коррелировать
  recipient/размер/время; он не читает payload и не подделывает проверяемую
  подпись.
- Direct-соединение раскрывает получателю IP-адрес отправителя на уровне Tor
  guard-цепочки, а не приложения; Tor не скрывает все метаданные.
- Один onion на идентичность чата; платёжный onion кошелька в этом проекте не
  используется и не смешивается с чатом.
- Логирование приложения не должно содержать plaintext/seed/ключи; CLI печатает
  только адреса, onion, id и сами сообщения (stdout — для пользователя).
- Проект не обещает анонимность, отсутствие ошибок или готовность к mainnet;
  это DEV/testnet-инструмент.

## 10. Совместимость

Wire-формат envelope v1 и gateway API (`/flint/v1/msg*`) совместимы с Flint
messenger v1; известные фиксированные векторы (шифрование, подпись, PoW, ACK)
перенесены в unit-тесты. Локальные форматы (identity-файл CLI, store) могут
отличаться от Flint и не считаются его частью. Платёжные Slatepack-сценарии
исходного Flint (автоматическая передача в кошелёк) в этот проект не входят:
`MessageKind::Slatepack` переносится как непрозрачный armor, без wallet-проверок.
