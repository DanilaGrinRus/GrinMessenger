# GrinMessageModule (extraction из Flint messenger) — отчёт о выделении (2026-11-07)

## Что сделано

Из публичного репозитория [DanilaGrinRus/Flint](https://github.com/DanilaGrinRus/Flint)
(commit `20efa26a953f83a36ec0fe248e390d0031d01a95`, 2026-10-30) выделен и
адаптирован самостоятельный мессенджер на Tor/Arti. Код кошелька, ноды, UI,
RPC и платёжной интеграции **не переносился**.

| Крейт | Назначение | Источник в upstream |
|---|---|---|
| `flint-messenger` | envelope v1, crypto, PoW, ACK, store/history/contacts/keywrap, транспорты, Nostr | `crates/messenger` |
| `flint-tor-arti` | in-process Arti: onion v3 из seed, onion service, HTTP-over-Tor | `third_party/grin-wallet/impls/src/tor/arti.rs` (Apache-2.0), `crates/core/src/onion.rs` |
| `flint-gateway` (`flint-relay`) | standalone store-and-forward relay без кошелька | `crates/core/src/gateway.rs`, `crates/core/src/bin/flint-relay.rs` |
| `flint-messenger-cli` (`flintmsg`) | keygen/address/serve/send/fetch | новый код; использует извлечённые крейты |
| `flint-messenger-ui` (`flintmsg-ui`) | локальный веб-UI: чат, контакты, envelope inspector, экспорт транскрипта | новый код (2026-11-08), не извлекался из Flint |

Лицензия: **MIT OR Apache-2.0** (владелец прав разрешил перелицензировать
выделенный код; Apache-2.0-части grin-wallet сохраняют уведомления). См.
[PROVENANCE.md](../../PROVENANCE.md), [THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md).

## Окружение и артефакты

| | |
|---|---|
| OS | Windows x64 (10.0.26100), PowerShell 7 |
| Rust | cargo/rustc 1.99.1 (MSVC, `x86_64-pc-windows-msvc`) |
| Сборка | offline, вендор `C:\DHW\vendor` через унаследованный `C:\DHW\.cargo\config.toml`; crates.io в окружении недоступен |
| GCP (live Tor) | Ubuntu 24.04 VM `grin-linux-build` (e2-medium, us-central1-a), Rust 1.99.1, публичный интернет; репозиторий клонирован с GitHub |
| Upstream-клон | `C:\DHW\tmp\flint-upstream` (read-only, не входит в поставку) |
| Версия | workspace `0.1.0`, `rust-version = 1.92` |
| Исходники | 37 `.rs`, 11 734 строк; `Cargo.lock` 556 пакетов; + один встроенный `ui.html` |
| Бинари (build output) | `target/debug/flintmsg.exe` 26.97 МБ, `target/debug/flint-relay.exe` 3.26 МБ, `target/debug/flintmsg-ui.exe` 27.06 МБ; **в поставку не входят, пересобираются из исходников** |
| Хэши upstream-файлов | [upstream-sources.sha256](upstream-sources.sha256), 40 файлов |
| Freeze-манифест поставки | [source-sha256.sha256](source-sha256.sha256), 59 файлов |

## Проверки

Все команды выполнялись из `C:\DHW\GrinMessageModule` (на момент extraction —
`FlintMessenger`) с `CARGO_TARGET_DIR=…\target`,
`TMP/TEMP=…\.tmp-build`, `--offline --locked`.

| # | Проверка | Результат | Лог |
|---|---|---|---|
| 1 | `cargo build --workspace --offline --locked` | **PASS** (EXIT_CODE=0) | [01-build.log](01-build.log) |
| 2 | `cargo test --workspace --offline --locked` | **PASS**: 125 passed, 0 failed, 3 ignored (EXIT_CODE=0) | [02-tests.log](02-tests.log) |
| 2a | из них `flint-messenger` lib | 89 passed, 1 ignored (Nostr live) | |
| 2b | `flint-gateway` lib | 19 passed | |
| 2c | `flint-tor-arti` lib | 5 passed (onion vectors, derivation) | |
| 2d | `flint-messenger-cli` tests | 3 (`cli_smoke`) + 9 (`loopback_e2e`) passed | |
| 3 | `cargo fmt --all -- --check` | **PASS** (EXIT_CODE=0) | [03-fmt.log](03-fmt.log) |
| 4 | `cargo test -p flint-gateway --test relay_live -- --ignored --test-threads=1` | **PASS**, 1 passed: реальный процесс `flint-relay`, quarantine битых envelope и restart-skip | [04-relay-live.log](04-relay-live.log) |
| 5 | Двухпроцессный loopback e2e: `flint-relay.exe` + `flintmsg keygen/send/fetch` | **PASS**: доставка, открытие, signed ACK, повторный fetch пуст | [06-e2e-demo.log](06-e2e-demo.log) |
| 6 | `cargo test -p flint-tor-arti --test live_onion -- --ignored --nocapture` (live Tor, исторический прогон) | **FAILED, NOT VERIFIED/BLOCKED** на тот момент: bootstrap timeout 300 с в локальной песочнице; позже live Tor проверен cross-process-прогоном на GCP (строка 17). | [05-live-tor.log](05-live-tor.log) |
| 7 | Чистая сборка из состава поставки (каталоги `target*`/`.tmp*` удалены) | **PASS**: `cargo build --workspace --offline --locked` 70.1 с, EXIT 0; `cargo test --workspace --offline --locked` 125 passed / 0 failed / 3 ignored, EXIT 0 | [07-clean-build.log](07-clean-build.log), [08-clean-tests.log](08-clean-tests.log) |
| 8 | Post-rename branding (`GrinMessageModule`, `repository` в манифестах) + повторная сборка/тесты/fmt | **PASS**: build 160 с EXIT 0; test 125/0/3 EXIT 0; fmt EXIT 0 | [09-rename-build.log](09-rename-build.log), [10-rename-tests.log](10-rename-tests.log), [11-rename-fmt.log](11-rename-fmt.log) |
| 9 | Смена имени репозитория на `GrinMessenger` (URL в README/PROVENANCE/Cargo.toml) + сборка/тесты/fmt | **PASS**: build 61 с EXIT 0; test 125/0/3 EXIT 0; fmt EXIT 0 | [12-repo-rename-build.log](12-repo-rename-build.log), [13-repo-rename-tests.log](13-repo-rename-tests.log), [14-repo-rename-fmt.log](14-repo-rename-fmt.log) |
| 10 | Английский перевод документации + лендинг `site/` + Pages workflow; повторная сборка/тесты/fmt | **PASS**: build 64 с EXIT 0; test 125/0/3 EXIT 0; fmt EXIT 0 | [15-en-site-build.log](15-en-site-build.log), [16-en-site-tests.log](16-en-site-tests.log), [17-en-site-fmt.log](17-en-site-fmt.log) |
| 11 | GitHub Pages: деплой сайта `site/` через workflow; проверка выдачи; удаление старого сайта Flint | **PASS**: run 42383391523 = success; https://danilagrinrus.github.io/GrinMessenger/ = HTTP 200; старый https://danilagrinrus.github.io/Flint/ = HTTP 404 (Pages отключён) | [18-pages.log](18-pages.log) |
| 12 | Flint: код убран, репозиторий переведён в архивную заглушку со ссылкой на GrinMessenger | **PASS**: в Flint остался только README.md-указатель; теги/релизы удалены, issues/projects/wiki выключены, archived=true | [19-flint-pointer.log](19-flint-pointer.log) |
| 13 | Веб-UI: сборка, тесты (`flint-messenger-ui`), fmt | **PASS**: build EXIT 0; tests **127 passed / 0 failed / 3 ignored** (включая 2 UI интеграционных), fmt EXIT 0 | [21-ui-build.log](21-ui-build.log), [22-ui-tests.log](22-ui-tests.log), [23-ui-fmt.log](23-ui-fmt.log) |
| 14 | Веб-UI: живое демо на loopback (relay + CLI-пир ↔ браузер), скриншот | **PASS**: 3 входящих + ответ из UI; CLI-пир получил ответ; signed ACK; история/контакты/экспорт; скриншот `docs/assets/ui.png` (1440×900) | [20-ui-demo.log](20-ui-demo.log), [20-ui-state.json](20-ui-state.json) |
| 15 | GitHub Pages: редеплой сайта с секцией Web UI и скриншотом | **PASS**: run 42425449844 = success (sha e2d4ae6); https://danilagrinrus.github.io/GrinMessenger/ = HTTP 200, секция «Web UI» и `assets/ui.png` на месте | [24-pages-ui.log](24-pages-ui.log) |
| 16 | GCP VM: клон опубликованного репозитория, сборка на Linux (Ubuntu, Rust 1.99) | **PASS**: `cargo build --workspace` 20m 58s (cold), инкрементальная пересборка после фиксов ~37 с | [23-gcp-build.log](23-gcp-build.log) |
| 17 | GCP VM: **live Tor/Arti, два реальных процесса** (`flintmsg serve` публикует onion через Arti; `flintmsg send` доставляет direct onion; получатель открывает и шлёт signed ACK) | **PASS** (2026-11-08): Arti bootstrap 100%, onion опубликован, `SEND_EXIT=0`, `RECEIVED=1`, signed ACK; плюс `cargo test --workspace` на Linux — 128/0/2 после фиксов | [25-live-tor-gcp.log](25-live-tor-gcp.log) |
| 18 | Найден и исправлен wire-баг live-режима: hyper отправлял absolute-form request-target, gateway отвечал 404; фикс + регрессионный тест | **PASS**: commit `d828211` (origin-form + Host; gateway принимает absolute-form), тест `absolute_form_request_target_is_routed_correctly`; после фикса direct onion доставка работает | [25-live-tor-gcp.log](25-live-tor-gcp.log) |

Итог на момент отчёта: **static/unit/build/manual loopback, веб-UI и live Tor
(cross-process direct onion на GCP VM) — PASS; известных P0–P2 дефектов нет;
локальная песочница сама по себе не имеет Tor-выхода, поэтому live-прогон
выполнен на GCP и привязан к коммиту.**

Freeze-манифест кода/доков/сайта: [source-sha256.sha256](source-sha256.sha256), его
SHA-256 — `b4b35bd9fdb48051692d6628f9d8abb1024fa9f7ab4083384d400c0c9db0318f`
([source-sha256.sha256.hash](source-sha256.sha256.hash)). `evidence/` намеренно
не входит в freeze-манифест и может дополняться VERIFY.md/логами.

> Примечание о VERIFY.md: независимая верификация выполнена на pre-publication
> freeze `487d5892…65f04` (каталог тогда назывался `FlintMessenger`). После неё
> изменены branding-файлы, поле `repository`, имена каталога/репозитория, затем
> добавлены английский перевод docs/`site/` и веб-UI (`crates/flint-messenger-ui`,
> 2026-11-08); Rust-код ядра не менялся, повторные build/test/fmt — логи 09–23,
> актуальный freeze-манифест — `b4b35bd9…0318f`. VERIFY.md оставлен как
> исторический артефакт.

### Что именно подтверждено

- сборка всего workspace offline из вендора, `--locked` без расхождений;
- 125 автотестов: протокол (envelope/crypto/PoW/TTL/ACK/квоты/quarantine),
  19 тестов relay (HTTP API, signed ACK, PoW, лимиты, restart-персистентность),
  onion-векторы Tor v3, loopback e2e двух идентичностей, CLI-процессы;
- ignored `relay_live`: реальный дочерний процесс `flint-relay`, битые
  envelope-ы в quarantine, restart skip;
- CLI: `keygen/address/send/fetch` с реальными процессами; повторный fetch после
  signed ACK не возвращает сообщение;
- onion-адрес из seed совпадает с Tor v3 вектором
  `2a6at2obto3uvkpkitqp4wxcg6u36qf534eucbskqciturczzc5suyid`;
- **live Tor/Arti**: на GCP VM два независимых процесса `flintmsg serve`/`send`
  провели direct onion доставку с signed ACK; попутно найден и исправлен
  wire-баг absolute-form (commit `d828211`) с регрессионным тестом.

### Что НЕ подтверждено (честно)

- **Live Tor**: в локальной песочнице Tor-выхода нет, поэтому первый прогон был
  NOT VERIFIED; на GCP VM (Ubuntu, публичный интернет) live-прогон двух реальных
  процессов прошёл: onion опубликован через Arti, direct-доставка и signed ACK
  подтверждены ([25-live-tor-gcp.log](25-live-tor-gcp.log)).
- **Direct-режим веб-UI**: требует Tor/Arti, в этом окружении не проверялся;
  проверен relay/loopback-режим (реальный браузер + реальный relay).
- **Публичный relay/Tor-сеть**: не разворачивались; relay проверен на loopback.
- **Аудит безопасности и mainnet**: вне задачи; forward secrecy нет, это
  DEV/testnet-инструмент.
- **Nostr-транспорт**: присутствует в библиотеке (перенесён), live-тест требует
  внешний relay и по умолчанию ignored; CLI/UI его не используют.
- **CLI не ведёт историю/контакты**; веб-UI сохраняет `history.json` и
  `contacts.json` в data-каталоге (без шифрования на диске — как и остальные
  служебные файлы UI-инструмента).

## Известные отличия от upstream (адаптация)

1. `flint-gateway`: удалён wallet proxy (`/v2/foreign`, `wallet_proxy`,
   `wallet_foreign_url`, `reqwest`); `POST /v2/foreign` теперь общий 404.
2. `flint-messenger`: добавлена абстракция `HttpConnector`/`TcpConnector` и
   конструкторы `with_connector`; `new`/`with_socks` и поведение HTTP сохранены.
3. `flint-tor-arti`: вместо внешнего `tor.exe`/SOCKS — in-process Arti;
   grin-специфичные `TorConfig`/`SecretKey`/`Error` заменены локальными типами;
   сроки bootstrap/request задаются в `TorConfig`.
4. Валидация адреса в relay — `flint_messenger::Address::parse` (эквивалент
   прежней проверки bech32 + hrp grin/tgrin + 32-byte key).
5. Документация адаптирована; wallet/Slatepack-автоматика удалена, а не
   перенесена как непроверяемое утверждение.
6. CLI `serve`: id сообщения помечается подтверждённым в памяти только после
   успешного signed ACK; при ошибке ACK повторяется на следующем poll.

## Исправлено по итогам независимой верификации (2026-11-07)

1. `LICENSE-MIT` — добавлена строка copyright (`Copyright (c) 2026 DanilaGrinRus`).
2. Битая ссылка `crates/flint-messenger/src/keywrap.rs` (`docs/historical/ARGON2ID.md`)
   и ссылки Nostr-модулей (`docs/contracts/MESSENGER.md`) заменены на
   `docs/PROTOCOL.md` / `docs/ARCHITECTURE.md`.
3. `docs/ARCHITECTURE.md` исправлены две неточности: gateway сам биндит loopback
   listener; CLI `serve` хранит неподтверждённые envelope-ы в file-backed
   mailbox gateway (`<data>/mailbox`), а не «не сохраняет на диск».
4. CLI `serve`: ACK-retry (см. отличия п.6) — найденный верификатором P3-дефект.
5. `evidence`: перегенерированы логи с `EXIT_CODE`, уточнены числа (556 пакетов,
   10 386 строк, 125/0/3 тестов), live Tor явно помечен FAILED/NOT VERIFIED.
6. `README.md` — статус и сводка тестов приведены к фактическим результатам.

## Остаток работ (не входит в extraction)

- Live Tor прогон на машине с доступом к сети Tor (два процесса `serve` + `send`),
  публикация onion-relay;
- независимый security review криптографии/протокола;
- оценка лицензий всех транзитивных зависимостей (на этой машине нет сканера;
  `Cargo.lock` — инвентарь);
- при публикации — git-репозиторий, CI, SBOM;
- UI/упаковка: не входят в этот проект (CLI + библиотека).

## Независимая верификация

Отдельный агент-верификатор проверил замороженное состояние независимо
(свой `target-verify`, повторные прогоны build/test/fmt/E2E, диффы к upstream,
аудит лицензий/секретов) и выпустил
[VERIFY.md](VERIFY.md) (SHA-256 `46c7d4591e87e4f006edfdb2d6aa18e17542f11ff028b256ec2baf4acbcb3bfc`),
логи — [verify-raw/](verify-raw/).

**Вердикт верификатора: готово как extraction, с оговоркой live Tor NOT
VERIFIED; известных P0–P2 дефектов на замороженном состоянии нет.**

- PASS: повторные fmt/build/test (125/0/3), ignored `relay_live` (реальный
  процесс), двухпроцессный loopback E2E, лицензии/provenance, содержательность
  выделения, отсутствие P0–P2.
- NOT VERIFIED/BLOCKED: live Tor (сеть Tor непригодна в окружении), direct-режим
  CLI (`serve`/`send` на onion), live Nostr (нужен внешний relay), release-сборка,
  security-аудит, не-Windows платформы.
- Freeze-привязка: все 50 хэшей `source-sha256.sha256` пересчитаны (50/50),
  SHA-256 манифеста совпал; upstream-манифест 40/40, клон HEAD `20efa26a…`, clean.
- 9 замечаний, найденных верификатором, исправлены и перепроверены (см. выше).
- Остаточные P3 (не блокеры): формулировка PROVENANCE без упоминания правок
  doc-комментариев; LICENSE-APACHE без необязательного APPENDIX; пустое
  `repository = ""`; сводка THIRD_PARTY_NOTICES без лицензионного сканера; перед
  публикацией — `cargo deny`/SBOM и очистка build-каталогов.

## Состав поставки

`C:\DHW\GrinMessageModule` — каталог с исходниками, `Cargo.toml`/`Cargo.lock`,
документацией, лицензиями и evidence; **~0.8 МБ**. Каталоги сборки (`target*`) и
временные (`.tmp*`) удалены; для запуска достаточно
`cargo build --workspace --offline --locked` (в окружении с вендором) или
`cargo build` на машине с crates.io. Upstream-клон
`C:\DHW\tmp\flint-upstream` и вендор `C:\DHW\vendor` — внешние, в поставку не
входят.

## Публикация

Модуль опубликован отдельным репозиторием **вместо Flint** (сам репозиторий
Flint не изменялся):

| | |
|---|---|
| Репозиторий | <https://github.com/DanilaGrinRus/GrinMessenger> (`GrinMessenger`; до переименования `GrinMessageModule`, старый URL редиректит) |
| Видимость | Public |
| Ветка по умолчанию | `main` |
| Initial commit | `e4a2c9d3fd16c1873e8566d33bca28c207a7e136` — «Initial import: GrinMessageModule — standalone Grin messenger over Tor/Arti (extracted from Flint messenger 20efa26a)» |
| Дата публикации | 2026-11-07 |
| Содержимое | только выделенный модуль (исходники, docs, лицензии, evidence); без кошелька/ноды/UI, без upstream-клона и вендора |
| Rename | проект/модуль = `GrinMessageModule`; репозиторий с 2026-11-07 = `GrinMessenger`; крейты/бинари сохраняют имена `flint-*` / `flintmsg` / `flint-relay` как provenance |

После переименования код не менялся: повторные сборка/тесты/форматирование —
логи 09–11 (PASS), новый freeze-манифест `11716d8a…4bd29`.

## Воспроизведение

```powershell
Set-Location C:\DHW\GrinMessageModule
$env:TMP = "C:\DHW\GrinMessageModule\.tmp-build"; $env:TEMP = $env:TMP
$env:CARGO_TARGET_DIR = "C:\DHW\GrinMessageModule\target"
cargo build --workspace --offline --locked
cargo test  --workspace --offline --locked
cargo test -p flint-gateway --test relay_live --offline -- --ignored --test-threads=1
```

Live Tor (если сеть доступна):

```powershell
cargo test -p flint-tor-arti --test live_onion --offline -- --ignored --nocapture
```
