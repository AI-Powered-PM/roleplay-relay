# Roleplay Relay

**RP chat helper.** A Windows tray app that helps a non-native English speaker roleplay in an
online game's text chat: it translates incoming lines and turns your own words into natural
English, ready to paste. Bring your own Anthropic or OpenAI API key — usage is billed to your account.

**It never touches the game.** It only reads and writes the clipboard, listens to global hotkeys
and shows its own windows. It never simulates input, installs input hooks, reads game memory or
files, or looks for the game window. You paste and press Enter yourself.

**Network calls.** The app talks to two places only: the AI provider you chose (Anthropic or
OpenAI), with your own key, and (optional, on by default) about one request a day to the GitHub
API for the number of the latest version (`Check for updates` in Settings). That check sends no
chat text, settings or keys, and the app never downloads or installs anything by itself.

Use at your own risk. No tool can guarantee your game account won't be actioned. You are responsible for every message sent from your account.

## Download and install

**Requirements**

- Windows 10 or 11 (64-bit).
- Your own Anthropic or OpenAI API key, with credits on that account.
- For World of Warcraft: windowed or borderless-windowed mode (exclusive fullscreen hides the overlay).

You do not need Rust, Node.js or any developer tools. Those are only for building from source (see
below). The installer adds Microsoft Edge WebView2 by itself if it is missing (it needs internet
during install).

**Steps**

1. Open <https://github.com/AI-Powered-PM/roleplay-relay/releases/latest>.
2. Under `Assets`, download `Roleplay.Relay_<version>_x64-setup.exe`.
3. Optional: check the file. In PowerShell run
   `Get-FileHash .\Roleplay.Relay_<version>_x64-setup.exe` and compare the result with the SHA-256
   in the release notes.
4. Run the installer. The installer is not code-signed, so Windows SmartScreen shows
   "Windows protected your PC". Click `More info`, then `Run anyway`.

Next: follow the [User Guide](docs/USER_GUIDE.md): add your API key, pick your language, set up the game.

What changed in each version: <https://github.com/AI-Powered-PM/roleplay-relay/releases>.

## Supported games

- World of Warcraft (profile `wow`).

The app knows each game's chat limit (255 characters for WoW) and splits long replies.
Everything else in the app is game-agnostic.

## Where data lives

Runtime files are in `%APPDATA%\RoleplayRelay\` (settings, prompts, characters, logs).
API keys are **not** there: each provider has its own entry in Windows Credential Manager, with
target names `anthropic-api-key.RoleplayRelay` and `openai-api-key.RoleplayRelay`.

## Building from source (developers)

Players do not need this section. Use the installer above.

See `CLAUDE.md` for the full spec. A game profile (`src-tauri/src/game.rs`) controls the chat limit
(255 characters), the most chat messages one reply may be split into (4), and the characters
stripped from outgoing text (`|`).

### Prerequisites (Windows 10/11 x64)

- Rust (stable, MSVC toolchain): <https://rustup.rs>
- Visual Studio Build Tools with the "Desktop development with C++" workload
- Node.js LTS (npm included)
- Microsoft Edge WebView2 Runtime (already present on Windows 11 and current Windows 10; it matters only for
  `npm run tauri dev` on an old Windows 10)

### Build and run

```bash
npm install
npm run tauri dev       # development run; the app starts hidden in the tray
npm run tauri build     # release build + NSIS installer
```

The installer is written to `src-tauri/target/release/bundle/nsis/` (`Roleplay Relay_<version>_x64-setup.exe`;
GitHub shows the uploaded file as `Roleplay.Relay_<version>_x64-setup.exe`).
The release build has devtools off and starts hidden in the tray.

### Tests

```bash
cd src-tauri && cargo test    # unit tests + the forbidden-API guard (tests/forbidden_apis.rs)
npm run build                 # TypeScript type check + production frontend build
```

Unit tests cover the Input guard, Output guard, splitter, settings repair, log rotation, hotkey parsing (tag neutralizing, secret sniffing, limits, debounce), prompt
building and prompt migration, the language table, and the API client against a local mock HTTP server (retry, error mapping,
temperature fallback). Nothing in the tests calls the real API.

`tests/forbidden_apis.rs` fails if any game-interaction identifier (input simulation, hooks,
process access, window lookup) appears in `src-tauri/src`. Do not weaken it.

### Adding a user language

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

### Security checks

```bash
cargo install cargo-audit     # once
cd src-tauri && cargo audit
npm audit --omit=dev
```

### Releasing

1. Bump the version to the same value in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and
   `package.json` (`Cargo.lock` and `package-lock.json` follow).
2. Create an annotated tag `vX.Y.Z` equal to that version.
3. Publish a normal release (not a draft, not a pre-release) from the `AI-Powered-PM` account with
   the NSIS installer and its SHA-256. The update check only sees normal releases whose tag looks
   like `vX.Y.Z`; otherwise clients never learn about it.

## Disclaimer

Roleplay Relay is not affiliated with or endorsed by Blizzard Entertainment. World of Warcraft and Warcraft are trademarks or registered trademarks of Blizzard Entertainment, Inc.
