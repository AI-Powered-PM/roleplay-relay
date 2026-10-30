# WoW Chat Helper

Windows tray app that helps a non-native English speaker roleplay in World of Warcraft chat.
It only uses the clipboard, global hotkeys and its own windows. It never touches the game.
See `CLAUDE.md` for the full spec and `docs/USER_GUIDE.md` for the player guide.

Status: **Milestone 1 (skeleton)** — tray icon, single instance, Settings window with a
write-only API key field, persisted settings. Translation and compose come in later milestones.

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

The installer is written to `src-tauri/target/release/bundle/nsis/`.

## Tests

```bash
cd src-tauri && cargo test    # unit tests + the forbidden-API guard (tests/forbidden_apis.rs)
npm run build                 # TypeScript type check + production frontend build
```

`tests/forbidden_apis.rs` fails if any game-interaction identifier (input simulation, hooks,
process access, window lookup) appears in `src-tauri/src`. Do not weaken it.

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
