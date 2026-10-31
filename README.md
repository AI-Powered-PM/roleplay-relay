# WoW Chat Helper

Windows tray app that helps a non-native English speaker roleplay in World of Warcraft chat.
It only uses the clipboard, global hotkeys and its own windows. It never touches the game.
See `CLAUDE.md` for the full spec and `docs/USER_GUIDE.md` for the player guide.

Status: **Milestone 5 (Polish)** — all MVP flows, live-configurable hotkeys, autostart, per-mode
model settings, size-rotating logs, NSIS installer.

## Prerequisites (Windows 10/11 x64)

- Rust (stable, MSVC toolchain): <https://rustup.rs>
- Visual Studio Build Tools with the "Desktop development with C++" workload
- Node.js LTS (npm included)
- Microsoft Edge WebView2 Runtime (already present on Windows 11 and current Windows 10)

## Build and run

```bash
npm install
npm run tauri dev       # development run; the app starts hidden in the tray
npm run tauri build     # release build + NSIS installer
```

The installer is written to `src-tauri/target/release/bundle/nsis/` (`WoW Chat Helper_0.1.0_x64-setup.exe`).
The release build has devtools off and starts hidden in the tray.

## Tests

```bash
cd src-tauri && cargo test    # unit tests + the forbidden-API guard (tests/forbidden_apis.rs)
npm run build                 # TypeScript type check + production frontend build
```

Unit tests cover the Input guard, Output guard, splitter, settings repair, log rotation, hotkey parsing (tag neutralizing, secret sniffing, limits, debounce), prompt
building and prompt migration, the language table, and the API client against a local mock HTTP server (retry, error mapping,
temperature fallback). Nothing in the tests calls the real API.

`tests/forbidden_apis.rs` fails if any game-interaction identifier (input simulation, hooks,
process access, window lookup) appears in `src-tauri/src`. Do not weaken it.

## Adding a user language

The user's language (setting `userLanguage`, dropdown `Your language`) is a static table in
`src-tauri/src/languages.rs`. To add one, add **one entry** to `LANGUAGES` (and bump the array
length). No other Rust logic and no bundled prompt needs to change. An entry has:

- `code` (stored in `settings.json`, e.g. `pt-BR`) and `label` (the language's own name, shown in
  the dropdown; the dropdown keeps table order),
- `english_name` (used in `Result still has {english_name} text`),
- `prompt_name` and `notes` (filled into `{{user_language}}` and `{{user_language_notes}}`),
- `leftover`: how a leftover is detected. `Leftover::Chars(fn)` for a script (Cyrillic, CJK; Japanese
  and Korean also need a CJK-capable font in the stack in `src/*/styles.css`), or
  `Leftover::Latin { letters, words }` for Latin-script languages (special letters, plus common
  words that only count when two distinct ones appear),
- `ok_samples` and `leftover_samples`: strings used by the unit tests.

The shared test in `languages.rs` then checks the entry against 20 real English RP lines (none may
be flagged) and against its `leftover_samples` (all must be caught). Add one test to
`outguard.rs` with the expected message if the language needs a specific wording. Then update
section 4 of `docs/USER_GUIDE.md` (language table and error table).

When you change a bundled prompt in `resources/prompts/`, add the SHA-256 of the outgoing default
(LF line endings) to `OLD_DEFAULTS` in `src-tauri/src/prompts.rs`, so unedited copies in users'
`%APPDATA%` are replaced on start. A test checks that every default shipped in git history is listed.

## Security checks

```bash
cargo install cargo-audit     # once
cd src-tauri && cargo audit
npm audit
```

## Where data lives

Runtime files are in `%APPDATA%\WowChatHelper\` (settings, prompts, characters, logs).
The API key is **not** there: it is stored in Windows Credential Manager under the
target name `anthropic-api-key.WowChatHelper`.
