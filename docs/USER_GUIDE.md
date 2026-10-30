# WoW Chat Helper — User Guide

This guide is for players. It uses short sentences. Names of buttons and messages are written
in `code style` exactly as they appear on screen.

> Status: this guide covers sections 1–3 so far. The other sections are filled in as
> the app grows.

## 1. What it is and what it never does

WoW Chat Helper is a small app that lives in the Windows tray (near the clock).
It helps you read and write English chat while you play.

It works only with:

- the **clipboard** (copy and paste),
- **hotkeys** (keys you press),
- **its own windows**.

It **never touches the game**. It does not press keys for you. It does not read the game's
memory or files. You paste the text into the game and press Enter yourself.

Why this matters: automatic input into the game can get your account banned. This app
is built so that it cannot do that.

You are responsible for every line you send. Always read the text before you press Enter.

## 2. Install and first run

1. Run the installer (`WoW Chat Helper_0.1.0_x64-setup.exe`). It installs for your user only.
2. Start **WoW Chat Helper**. No window opens. A small icon appears in the tray.
   If you do not see it, click the arrow `^` next to the clock.
3. Click the tray icon to open the menu:
   - `Show/Hide overlay` — not available yet (grayed out).
   - `Character` — shows `default`. More characters come later.
   - `Settings` — opens the Settings window.
   - `Quit` — closes the app.
4. Start the app only once. If you start it again, it just opens `Settings`.
5. Closing the Settings window does not close the app. Use `Quit` in the tray menu.

Start with Windows: this option is not available yet.

## 3. API key

The app uses the Claude API from Anthropic. You need your own API key.

**Create a key**

1. Open the Anthropic Console: <https://console.anthropic.com>
2. Sign in. Add a small amount of credit in the billing section.
3. Open the API keys page and create a new key. Copy it.

**Put the key in the app**

1. Tray icon → `Settings`.
2. In the `Anthropic API key` section, click the field with the hint `Paste your API key`
   and paste the key (Ctrl+V). You will see dots, not the key.
3. Click `Save key`.
4. The text under the title changes to `API key is set`, and you see `Key saved`.
   The field is empty again. This is normal: the app never shows the key again.

**Replace the key**: paste the new key and click `Save key`.

**Remove the key**: click `Clear key`. You will see `Key removed` and `No API key set`.

**Where the key is stored**: in Windows Credential Manager (Control Panel → Credential Manager →
Windows Credentials). It is not in any app file.

**Important**: after you copy the key, it stays in the Windows clipboard. Clear it:
copy any other short text (for example a single letter). The app also refuses to send
anything that looks like a key.

If you see `That does not look like an API key`, the pasted text is empty, too long,
or contains spaces. Copy the key again, without extra text.

If you see `Could not access Windows Credential Manager`, restart the app. If it
continues, restart Windows.

Settings you can already change in the Settings window: overlay `Font size`, `Opacity`,
`Auto-hide (seconds)`, and `Log chat text (debugLogChat)`. Click `Save settings`
(you see `Settings saved`). The overlay itself comes in the next milestone.
The buttons `Open prompts folder`, `Open characters folder` and `Open logs folder`
open the matching folder in `%APPDATA%\WowChatHelper\`.

## 4. Game setup

_Coming in a later milestone._

## 5. How to use it

_Coming in a later milestone._

## 6. Hotkey cheat sheet

_Coming in a later milestone._

## 7. Settings

_Coming in a later milestone (the full field-by-field list)._

## 8. Character, glossary and prompts

_Coming in a later milestone._

## 9. Error and block messages

_Coming in a later milestone._

## 10. Troubleshooting

_Coming in a later milestone._

## 11. Costs

_Coming in a later milestone._
