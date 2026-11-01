# Change: clearer hotkey labels in Settings

Do this as its own change on top of the released v0.1.0. One commit for code + spec + docs.

## Problem

The Settings → `Hotkeys` block is not self-explanatory:

- `Translate clipboard` and `Quick clipboard mode` look like the same thing. Neither label says the
  direction (incoming EN → user language vs. outgoing → English) or what happens to the clipboard.
- `Quick clipboard mode` does not say what it does at all, and it hides state: the user cannot see
  which mode (Direct / RP / Grammar) it will use.

## Goal

A user who reads only the Settings window understands what each hotkey does, which direction it
translates, and whether it changes the clipboard.

## What does NOT change

- Hotkey defaults, behavior, action function names (`translate_incoming`, `compose_submit`,
  `quick_clipboard`, `next_part`), event names, `settings.json` keys. No settings migration.
- The existing help line under the block
  (`Click a field and press the new combination. Use at least one of Ctrl, Alt, Shift. Changes apply when you press Save settings.`).
- Conflict marking (red) and `Reset` buttons.
- In code and in CLAUDE.md the internal term stays "compose". Only user-facing text changes.

---

## 1. Settings → Hotkeys block

Split the block into three small sub-headings. Under each hotkey row, add one hint line in the same
style as the existing help line (small, muted), aligned with the label.

```
Hotkeys

Reading chat
  Translate copied line   [Ctrl+Alt+T] [Reset]
    Copy someone's chat line first. Shows the {language} translation in the overlay. Your clipboard stays the same.

Writing replies
  Open reply window       [Ctrl+Alt+R] [Reset]
    Type in any language. The English result goes to your clipboard.
  Convert copied text     [Ctrl+Alt+E] [Reset]
    Copy your own text first. The English result replaces your clipboard. Uses {mode} mode, the last mode you picked in the reply window.
  Copy next part          [Ctrl+Alt+N] [Reset]
    For replies longer than {chat_limit} characters. Copies the next part.

Overlay
  Show/Hide overlay       [Ctrl+Alt+O] [Reset]
    Shows your last 10 messages and translations.

Click a field and press the new combination. ... (unchanged)
```

| Old label | New label | Sub-heading |
|---|---|---|
| `Translate clipboard` | `Translate copied line` | `Reading chat` |
| `Compose reply` | `Open reply window` | `Writing replies` |
| `Quick clipboard mode` | `Convert copied text` | `Writing replies` |
| `Copy next part` | `Copy next part` (unchanged) | `Writing replies` |
| `Show/Hide overlay` | `Show/Hide overlay` (unchanged) | `Overlay` |

### Dynamic values in hints

- `{language}` — the **English name** of the language currently selected in the `Your language`
  dropdown (`German`, `Chinese`, `French`, `Spanish`, `Russian`; from `languages.rs` via
  `list_languages()`). Updates live when the dropdown changes, before `Save settings`.
- `{mode}` — display name of the last used outgoing mode: `Direct`, `RP` or `Grammar`, read from the
  stored last-mode setting. Refresh it when the Settings window gains focus (the user may have switched
  mode in the reply window while Settings was open).
- `{chat_limit}` — the active game profile's `chat_limit`. The frontend must not hardcode `255`: get it
  from the backend (extend the existing settings-state command, or add a read-only
  `get_game_profile()` returning `{ id, display_name, chat_limit }` and allow it in the `settings`
  capability only).

Hints are plain text. Labels and hints are English for every user language (UI stays English).

## 2. Other user-facing text

- Grep `src/` and `src-tauri/src/` for the old labels and for user-facing words "compose" /
  "quick clipboard". Any text the user can see uses the new names: e.g. the quick-mode block message
  in the overlay that suggests compose should point to `Open reply window`.
- If such a message names a hotkey, show the **currently configured** combo, not the default.
- Tray menu is unchanged (`Show/Hide overlay`, `Character ▸`, `Settings`, `Quit`).

## 3. CLAUDE.md edits

- **User flows** headings:
  - `### 1. Incoming: translate EN → user language (Ctrl+Alt+T)` → add `UI label: Translate copied line`.
  - `### 2. Outgoing: compose (Ctrl+Alt+R)` → add `UI label: Open reply window. The compose window is called "reply window" in all UI text and docs.`
  - `### 3. Quick clipboard mode (Ctrl+Alt+E) — fallback flow` → `### 3. Convert copied text (Ctrl+Alt+E) — quick mode, fallback flow`.
- After flow 5, add a subsection **"Hotkey labels in Settings"** with the block layout, label table and
  dynamic-value rules from section 1 of this file.
- **Output guard**, block step: "in quick mode the overlay shows the reason and suggests compose" →
  "...and suggests `Open reply window`".
- **Documentation**, USER_GUIDE section 6: "quick clipboard mode" → "convert copied text"; section 7:
  "the action column uses the exact Settings labels".
- **Acceptance checklist**: add the items from section 5 below under a new group "Settings UX".

## 4. Docs

- `docs/USER_GUIDE.md` quotes UI labels exactly, so update:
  - section 6 (How to use it): subsection titles use the new labels; explain the difference between
    `Translate copied line` (read someone else's line, clipboard unchanged) and `Convert copied text`
    (send your own line, clipboard replaced) in two short sentences;
  - section 7 (Hotkey cheat sheet): new labels, grouped like Settings;
  - section 8 (Settings): describe the sub-headings and hint lines;
  - section 10 (Errors): any message text that changed;
  - any screenshot of the Hotkeys block.
- `README.md`: if it lists hotkeys, use the new labels.
- Release notes of the next version: one line — "Settings: clearer hotkey names and hints. `Translate clipboard` → `Translate copied line`, `Compose reply` → `Open reply window`, `Quick clipboard mode` → `Convert copied text`. Hotkeys themselves did not change."

## 5. Acceptance

- [ ] Settings shows the three sub-headings and the five rows with the new labels and hint lines.
- [ ] Changing `Your language` to `简体中文` → the first hint says `Chinese translation` immediately,
      before `Save settings`.
- [ ] Pick RP in the reply window, then open Settings → the `Convert copied text` hint says `RP mode`.
      Switch to Grammar in the reply window, focus Settings again → `Grammar mode`.
- [ ] `Copy next part` hint shows the profile limit; with a test profile limit of 200 it shows `200`.
      `255` still appears only in `game.rs` (and tests).
- [ ] Old labels `Translate clipboard`, `Compose reply`, `Quick clipboard mode` appear nowhere in
      `src/`, `src-tauri/src/`, `docs/`, `README.md`.
- [ ] Existing `settings.json` from v0.1.0 loads unchanged; all hotkeys keep their combos.
- [ ] Hint lines wrap cleanly at the minimum Settings window width; no horizontal scroll.
- [ ] USER_GUIDE sections 6–8 match the Settings window word for word.
- [ ] `npm test`, `cargo test`, `cargo clippy -D warnings` pass.
