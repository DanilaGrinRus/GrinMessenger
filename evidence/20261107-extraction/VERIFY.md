# Независимая верификация FlintMessenger (task-4) — 2026-11-07

Верификатор: `verifier` (независимый adversarial reviewer). Код и docs не менялись;
записаны только `evidence/20261107-extraction/VERIFY.md` и логи `verify-raw/`.

## Итоговый вердикт

**Готово как extraction: да, с оговоркой.** На замороженном состоянии
(манифест `source-sha256.sha256`, его SHA-256 = `487d58926a7a66d67011047a72f784449d287cb3c0b231a75b0b70dc14865f04`)
offline-сборка, 125 автотестов, ignored relay-live и двухпроцессный loopback E2E —
**PASS**; лицензии/provenance — **PASS**; live Tor (onion через Arti и direct-режим
CLI) — **FAILED/NOT VERIFIED/BLOCKED** из-за недоступности сети Tor в окружении,
это не выдача мока за PASS. Известных P0/P1/P2-дефектов в коде/доках на финальном
состоянии не найдено; остаются P3-замечания (ниже).

Привязка: все проверки выполнены на финальных файлах; независимо пересчитаны
все 50 хэшей freeze-манифеста (совпали 50/50, `verify-raw/13-freeze-manifest-check.log`),
хэши upstream-манифеста (40/40, `verify-raw/14-upstream-provenance-check.log`),
upstream-клон `C:\DHW\tmp\flint-upstream` — HEAD `20efa26a953f83a36ec0fe248e390d0031d01a95`, clean.

## Что было в scope и чем проверялось

- Источник истины — код, данные и воспроизводимые команды; выводы Lead/REPORT не
  принимались на слово: build/test/fmt/E2E/live-прогоны перезапущены лично.
- Своё окружение: `TMP/TEMP=C:\DHW\FlintMessenger\.tmp-verify`,
  `CARGO_TARGET_DIR=C:\DHW\FlintMessenger\target-verify`, offline-вендор
  `C:\DHW\vendor` через `C:\DHW\.cargo\config.toml`; cargo/rustc 1.99.1 MSVC.
- Логи: `evidence/20261107-extraction/verify-raw/` (полные команды, exit codes,
  stdout/stderr); до-фиксовые прогоны сохранены отдельно (`05a-e2e-demo-pre-freeze.log`
  и т.п.) — финальные прогоны выполнялись уже по замороженному манифесту.

## 1. Сборка и тесты — PASS

Финал (логи `verify-raw/08…12`, привязаны к freeze-манифесту):

| Команда | Exit code | Результат |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | PASS (`08-fmt-final.log`) |
| `cargo build --workspace --offline --locked` | 0 | PASS, `Finished dev` 4.25 s (`09-build-final.log`) |
| `cargo test --workspace --offline --locked` | 0 | **125 passed, 0 failed, 3 ignored**, 23.8 s (`10-tests-final.log`) |
| `cargo test -p flint-gateway --test relay_live --offline -- --ignored --test-threads=1` | 0 | **1 passed, 0 failed**, 5.28 s, реальный `flint-relay.exe` (`11-relay-live-final.log`) |

Разбивка 125 = 19 gateway + 89 messenger + 3 cli_smoke + 9 loopback_e2e + 5
tor-arti; 3 ignored = `relay_live` (запущен отдельно, PASS), Nostr live (нужен
внешний relay), `live_onion` (live Tor, FAILED/NOT VERIFIED — см. п.5).
Совпадает с ожиданием Lead (125/0), но проверено лично, а не принято на слово.

## 2. Лицензии и provenance — PASS (с P3-замечаниями)

- `MIT OR Apache-2.0`: root `Cargo.toml:14`, license у всех 4 крейтов
  (workspace), `LICENSE-MIT` (канонический текст MIT + добавленный
  `Copyright (c) 2026 DanilaGrinRus`), `LICENSE-APACHE` (канонические Terms
  Apache-2.0; упрощённая нормализованная сверка с текстом из вендора — 150/150
  непустых строк совпали; не включён необязательный APPENDIX «How to apply» —
  P3).
- `NOTICE`, `THIRD_PARTY_NOTICES.md`, `PROVENANCE.md` заполнены; upstream
  remote+commit `20efa26a…` указан в PROVENANCE/README/REPORT; происхождение
  grin-wallet (Apache-2.0) отмечено в headers `arti.rs`, `keys.rs`, `onion.rs`
  и в NOTICE/THIRD_PARTY_NOTICES; upstream-манифест `upstream-sources.sha256` —
  40/40 хэшей совпали с read-only клоном.
- SPDX: `// SPDX-License-Identifier: MIT OR Apache-2.0` присутствует в 35/35 `.rs`.
  GPL-заголовков в коде нет; GPL упоминается только в NOTICE/PROVENANCE как
  история перелицензирования (законно и требуется).
- Выдуманных авторов/копирайтов нет: `DanilaGrinRus` — владелец upstream-remote;
  per-file copyright-строк в коде нет.
- Секреты: `seed_hex` встречается только в `crates/flint-messenger-cli/src/main.rs`
  (формат identity-файла) и `docs/PROTOCOL.md` (документация формата); в
  `evidence/` seed/ключей нет. 64-hex строки в `crates/` — фиксированные тестовые
  векторы (`000…01`), допустимы. Временные каталоги `.tmp-demo/` и мой
  `.tmp-verify/` содержат одноразовые CLI-идентичности (сгенерированы
  `getrandom` для демо, не пользовательские seed); в поставку/evidence не входят,
  но перед упаковкой temp-каталоги нужно вычистить (P3, см. «Риски»).
- Зависимости: `cargo metadata --offline --locked` — 556 пакетов; GPL-only нет;
  `arti-client`/`tor-*` ровно 0.43.0 (40 пакетов); `reqwest`/`grin*`/`wallet*`
  в графе отсутствуют; `rg`-скан по коду: нет wallet proxy/foreign/reqwest
  (кроме комментария об удалении и теста, что `/v2/foreign` → 404).

## 3. Содержательность выделения — PASS

- Upstream `crates/messenger` → `crates/flint-messenger`: файловый состав совпадает
  (единственное отличие — `tests/relay_live.rs` перенесён в gateway, что
  задокументировано в PROVENANCE). `#[test]`: upstream 85 → extracted 90
  (+5 новых тестов `HttpConnector`), ни один upstream-тест не потерян/не ослаблен;
  `#[ignore]` — 1:1.
- "Byte-identical после SPDX" проверено `git diff --no-index`: все файлы —
  только +1 строка SPDX, кроме явно заявленных: `transport.rs` (302/59 —
  абстракция `HttpConnector`/`TcpConnector`), `lib.rs` (2/1 — экспорт),
  `Cargo.toml` (замена `workspace = true` на версии; версии/фичи сверены с
  upstream workspace — совпадают), и 3 файлов с правкой ТОЛЬКО doc-комментариев
  (`keywrap.rs` 7/7, `nostr/mod.rs` 2/2, `nostr/nip44.rs` 3/2 — это устранённые
  битые ссылки на несуществующие в поставке docs; логика не менялась).
- `HttpConnector` не сломал поведение `GatewayTransport`: в диффе сохранены
  грамматика URL и порядок валидации, `Host`/`Content-Length`/`Connection: close`,
  `MAX_RESPONSE`, таймауты `TIMEOUT`/`ONION_TIMEOUT`, разбор статуса/ошибок;
  `new`/`with_socks` делегируют в те же проверки; 5 новых тестов проверяют
  делегирование абсолютных URL, fetch/ack/`DirectOnionTransport`, старый контракт
  `url_and_proxy_validation_keeps_the_old_contract`. Регрессий не найдено.
- Gateway: wallet proxy (`/v2/foreign`), `wallet_proxy`/`wallet_foreign_url`,
  `reqwest`, `State.http` удалены; API сообщений/PoW/квоты/TTL/signed ACK
  сохранены; `to` валидируется `flint_messenger::Address::parse` — логика
  идентична upstream `onion_from_address` (bech32 + hrp grin/tgrin + 32-byte key).
  Runtime-проверка: `GET /flint/v1/health` 200, `POST /v2/foreign` 404,
  `GET /flint/v1/msg?to=bogus` 400, `POST /flint/v1/msg {}` 400,
  `POST /flint/v1/msg/not-a-uuid/ack` 400.
- `flint-tor-arti` действительно in-process Arti: `arti-client 0.43.0` +
  `tor-hsservice/hsrproxy/keymgr/llcrypto/hscrypto 0.43.0`; в коде нет
  `Command::new`/запуска `tor.exe`/`TorProcess` (единственное упоминание
  `std::process::id()` — для temp-каталога теста). Адаптация `impls/src/tor/arti.rs`
  (bootstrap/keystore/onion-service/reverse-proxy/state-lock) сверена с upstream —
  механика эквивалентна, grin-типы заменены локальными; `TorConfig` содержит все
  требуемые публичные поля (`state_dir`, `bootstrap_timeout`, `request_timeout`,
  `bridge_line`, `bridge_bin_path`), внешняя настройка через `TorConfig::new` +
  `with_bridge`/публичные поля подтверждена компиляцией и прогоном CLI.
- Onion-вектор: seed `d03c09e9…c8bb` → `2a6at2obto3uvkpkitqp4wxcg6u36qf534eucbskqciturczzc5suyid`
  подтверждён юнит-тестом (`key_vector_matches_the_reference_onion`, TEST PASS) и
  сквозной проверкой `onion_address_matches_tor_arti_derivation` в loopback_e2e.
- CLI: `keygen/address/send/fetch` реально работают с настоящими процессами (мой
  E2E); `serve` в direct-режиме требует Tor и в этом окружении не запускался —
  NOT VERIFIED (см. п.5), не PASS. Заглушек/фиктивных PASS в коде нет; найденные
  `fake_socks`/`stub` — тестовые эмуляторы SOCKS/локального HTTP для юнит-тестов,
  live-тест onion реально поднимает Arti (не мок).
- README-пример библиотеки проверен компиляцией отдельного крейта
  (`verify-raw/07-readme-example.log`, exit 0).

## 4. Loopback E2E (реальные процессы, не mock) — PASS

Финальный прогон `verify-raw/05b-e2e-demo-final.log` (бины из
`target-verify/debug`, sha256: `flintmsg.exe` `73ca2c81…24a5b`,
`flint-relay.exe` `cfc0d4f4…dc08`), 0 FAIL-строк:
- отдельный процесс `flint-relay.exe` на loopback, health-check OK;
- keygen 2 идентичностей и `address` (address/onion round-trip);
- `send --relay … --plain`: сообщение доставлено, id = UUID;
- в relay-store ровно 1 файл `<id>.json`, payload opaque: нет plaintext-метки и
  адреса отправителя; виден только `to` (метаданные по дизайну);
- unsigned ACK → HTTP 401 и сообщение НЕ удалено;
- `fetch`: сообщение открыто (text/from/id совпали), signed ACK `count=1`,
  файл из store удалён;
- повторный `fetch` пуст (`opened=0`);
- relay остановлен: `send`/`fetch` завершаются exit=1 (ошибка подключения, не
  молчаливый успех);
- `send --plain` на onion и `fetch` `.onion` без Arti корректно отклоняются.

## 5. Live Tor — FAILED / NOT VERIFIED / BLOCKED

- Проектный лог `05-live-tor.log`: `live_onion_service_roundtrip` FAILED после
  300.02 s — `onion service start: tor: bootstrap timeout (300s); проверьте сеть/Tor`.
- Мой независимый bounded-прогон (`verify-raw/06-live-tor-independent.log`):
  тот же результат — bootstrap timeout 300 s, wall 322.6 s, exit FAILED.
- Причина (факты, замеры 19:11): TCP `1.1.1.1:443`/`9.9.9.9:443` — CONNECTED;
  некоторые Tor ORPort :443 доступны, но Tor dir-auth `128.31.0.39:9101/:443` — ERR,
  `86.59.21.38:443`/`194.109.206.212:443` — нет соединения; локального SOCKS
  `127.0.0.1:9050` нет; proxy-env нет; TLS-запрос к `https://check.torproject.org`
  не установился (SSL handshake).
- Вывод: сеть Tor в этом окружении непригодна (блокировка/фильтрация); нельзя
  отличить сетевую политику от Tor-специфичного ограничения. **PASS не заявляется;
  mock за live не выдаётся.** Прямой режим `flintmsg serve`/`send` на onion — NOT
  VERIFIED по той же причине. Непроверенным остаётся и live-путь `TorHttpClient`
  (успешный bootstrap в этом окружении недостижим в принципе).

## 6. Adversarial review — найденные дефекты и их статус

Все замечания были переданы Lead и исправлены; после фиксов проверено лично:

| # | Было (severity) | Где | Проверка после фикса |
|---|---|---|---|
| 1 | REPORT утверждал fmt PASS при логе с diff (P2, несоответствие evidence) | `REPORT.md`, `03-fmt.log` | новый `03-fmt.log` = `EXIT_CODE=0`; мой `08-fmt-final.log` = 0 |
| 2 | REPORT: «2 ignored» (P3) | `REPORT.md:43` | теперь «3 ignored»; мой прогон = 3 |
| 3 | REPORT: 552 пакета / ~9515 строк / 28.3+3.4 МБ (P3) | `REPORT.md:30-31` | 556 / 10 386 / 26.97+3.22 МБ; сверено |
| 4 | Live Tor не был явно помечен FAILED (P3) | `REPORT.md:51`, README | явно FAILED/NOT VERIFIED/BLOCKED |
| 5 | `01-build.log` без exit code (P3, доказательность) | evidence | перегенерирован с `EXIT_CODE=0`; 02/04 тоже |
| 6 | `LICENSE-MIT` без copyright (P3, MIT-гигиена) | `LICENSE-MIT` | строка `Copyright (c) 2026 DanilaGrinRus` добавлена |
| 7 | Битые ссылки на docs (P3) | `keywrap.rs:18,48`, `nostr/mod.rs:4`, `nostr/nip44.rs:4-5` | ссылки на `docs/PROTOCOL.md`/`ARCHITECTURE.md`; файлы существуют, логика не менялась |
| 8 | ARCHITECTURE: неточности про gateway/CLI-историю (P3) | `docs/ARCHITECTURE.md:18,87` | исправлено: gateway сам биндит listener; mailbox file-backed |
| 9 | CLI `serve`: ACK после ошибки не повторялся (P3) | `main.rs` serve-loop | id добавляются в память только после успешного ACK; повтор на следующем poll; код перечитан |

Остаточные P3-замечания (не блокируют extraction, рекомендуется учесть):
- `PROVENANCE.md:39`: формулировка «copied, SPDX headers added…» не перечисляет
  правки только doc-комментариев в 3 файлах (логика не менялась — проверено диффом);
  можно уточнить одной фразой при следующем обновлении.
- `LICENSE-APACHE`: отсутствует необязательный APPENDIX «How to apply»; Terms
  Apache-2.0 полные (150/150 строк сверены) — юридически достаточно, но канонический
  файл обычно включает appendix.
- `Cargo.toml:16`: `repository = ""` — пустое метаданное; при публикации крейтов
  указать реальный URL (не влияет на сборку).
- `THIRD_PARTY_NOTICES.md`: «mostly MIT/Apache/BSD/ISC» — сводка; в дереве есть
  dual-licensed `priority-queue 2.7.0` (LGPL-3.0-or-later OR MPL-2.0) и
  `r-efi` (MIT/Apache/LGPL) — выбираемыми являются MIT/Apache/MPL-опции; перед
  публикацией рекомендуется `cargo deny`/`cargo about` (в offline-окружении
  сканера нет, `Cargo.lock` — инвентарь). GPL-only зависимостей нет.
- `crates/flint-tor-arti/src/lib.rs:35`: дефолт bootstrap-таймаута крейта 120 s,
  CLI ставит 150 s, live-тест — 300 s; это настраиваемые значения, README говорит
  «таймаут настраивается в коде» — противоречия нет, но при документировании
  удобно назвать числа.
- Перед упаковкой поставки вычистить неприкладываемые каталоги `.tmp-*/` и
  `target*/` (в `.gitignore` они есть, но каталог поставки — не git-репозиторий);
  внутри `.tmp-demo/` лежат одноразовые демо-идентичности (не пользовательские
  секреты, в evidence не попадают).

## Что осталось непроверенным (и почему)

- **Live Tor / onion service / direct-режим CLI** — сеть Tor недоступна (см. п.5).
- **Live Nostr-транспорт** — тест `#[ignore]` требует внешний relay (сети нет).
- **Публичный relay / публикация onion-relay** — не разворачивались.
- **Release-сборка** (`cargo build --release`, README) — не запускалась (не
  требовалась задачей; dev-сборка проверена).
- **Аудит безопасности/криптографии и нагрузочное тестирование** — вне scope
  extraction; существующие тесты покрывают negative-кейсы (tamper, ACK, TTL,
  квоты, quarantine, restart), но не заменяют security review.
- **Платформы кроме Windows x64 MSVC** — не проверялись.
- **Лицензионный скан всех транзитивных зависимостей** — см. P3 выше.

## Воспроизведение (verifier)

```powershell
Set-Location C:\DHW\FlintMessenger
$env:TMP = "C:\DHW\FlintMessenger\.tmp-verify"; $env:TEMP = $env:TMP
$env:CARGO_TARGET_DIR = "C:\DHW\FlintMessenger\target-verify"
cargo fmt --all -- --check
cargo build --workspace --offline --locked
cargo test  --workspace --offline --locked
cargo test -p flint-gateway --test relay_live --offline -- --ignored --test-threads=1
& .\evidence\20261107-extraction\verify-raw\e2e-demo.ps1        # V2: реальные процессы, 0 FAIL
& .\evidence\20261107-extraction\verify-raw\live-tor-attempt.ps1 # ожидаемо FAILED: bootstrap timeout
```

## Артефакты верификации (sha256)

| Файл | sha256 | Размер |
|---|---|---|
| `source-sha256.sha256` (freeze, 50/50 совпало) | `487d58926a7a66d67011047a72f784449d287cb3c0b231a75b0b70dc14865f04` | 5001 |
| `source-sha256.sha256.hash` | `5fa6cf09f32cf6966c407952696d2fc25b72acef99ace4ee02d21b83f6daf16f` | 88 |
| `upstream-sources.sha256` (40/40 совпало) | `e3e4bb42735b5192d408da4af7ac8d01cc4ff21daf6ba6457e11c9ae4c31193a` | 4072 |
| `01-build.log` | `cf29b9b6cf1fcb1f16cf76234f79f909cdfe73eb276a1ed502c79a06f67556f3` | 361 |
| `02-tests.log` | `76435d54d43fffa2b8b973d4951a28c6819a8f978396bf4a4d2fd93573d62c6d` | 11427 |
| `03-fmt.log` | `59d6101cdb500ace98037f542f831fb892e45f1793cd5b0821364f75ef18b873` | 32 |
| `04-relay-live.log` | `8cb28b2c732b8ffbd872e470563e27016b2e368a986933455b71f9ff97817950` | 556 |
| `05-live-tor.log` | `858447477b139fecb077dcf166b05614eecb2b17fe2548f2962530dc17a571c5` | 1047 |
| `06-e2e-demo.log` | `68abf4bf110ee1fc0a55269b13cc80af247230d8b1a03d4ca6670e9bba3510eb` | 1912 |
| `REPORT.md` | `509e06044f1eae1a3612054b2e7877c5775631969e892d6261ff69e98440885c` | 10569 |
| `verify-raw/08-fmt-final.log` | `6c6e52c7d48d7190e2f363aa6b3fd17fc055c7f6f05db0d4347b4cc3369d1c0a` | 194 |
| `verify-raw/09-build-final.log` | `47c8f401325935ef011b58ce85e7a65bbda5abb2ea942e9ec0479e72f6417a3f` | 527 |
| `verify-raw/10-tests-final.log` | `14278ed16f25db7a705823e911b6081f51154ee8020ddd7c1e54d0bd3fd34f12` | 11655 |
| `verify-raw/11-relay-live-final.log` | `3e7c4b2a809a36d8d7dde0ba3a1f8a736ad3b0df2d270f1cd527d76d05822a8f` | 766 |
| `verify-raw/12-final-summary.log` | `3518a88b1ccf0ff0d0a84357f63dee8df5fb2886689ab5e32052668971c28fbb` | 56 |
| `verify-raw/13-freeze-manifest-check.log` (50/50, hash-file совпал) | `0300568c3fa2aab68d25ef583127f91efface5c78070641c1dd667108d24a05d` | 362 |
| `verify-raw/14-upstream-provenance-check.log` (40/40, HEAD/dirty) | `0fcb7ed29e53655ab7a2c5e6068851e002930be6fb6de991faf63ceeaaaa8282` | 345 |
| `verify-raw/13-freeze-manifest-files.txt` (список 50 файлов freeze) | `077ba493943f86464a8c9195ac44b54baf905ddcb5e30bffebc763964fd5a642` | 1701 |
| `verify-raw/05b-e2e-demo-final.log` | `59bdce093a1d0502d54998737d0d710b84d22e7a584ef3640c1518f539ed4f44` | 4667 |
| `verify-raw/05a-e2e-demo-pre-freeze.log` | `43db7830974955af02644556c89ccf54113a6d26d2cee8b235c426b8d9b19195` | 4669 |
| `verify-raw/06-live-tor-independent.log` | `9c1cf531cfe7de047b505374ef18694ff82e7c940cec6ae7cabf9fd063ba6ec4` | 4143 |
| `verify-raw/07-readme-example.log` | `76dfb0cb1f69c1ff3a301b062dbc20cf4fdaa577aa0bb2b9e521751efd47bf1e` | 4183 |
| bin `target-verify/debug/flintmsg.exe` | `73ca2c8188a89140bd69b02ef300ad6db40d8111ca743b346607967110824a5b` | 26.97 MB |
| bin `target-verify/debug/flint-relay.exe` | `cfc0d4f418742c6da4b9207c1582b969e7697c90f4497c35919c48735cabdc08` | 3.22 MB |

Проектные evidence-логи финального состояния (`01-build.log` … `06-e2e-demo.log`,
`REPORT.md`, `upstream-sources.sha256`) перечитаны и согласованы с моими прогонами;
`evidence/` намеренно не входит в freeze-манифест кода/доков.
