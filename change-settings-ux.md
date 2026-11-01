# Change: Settings window UX pass

Do this together with `change-hotkey-labels.md` as **one change** on top of v0.1.0 (its section 1 is the
`Hotkeys` card below). One commit for code + spec + docs.

## Problem (current window, v0.1.0)

1. No answer to "am I ready?": key status is small text; there is no way to check that a key works.
2. Order does not follow first run: `General` (autostart) sits between the keys and `Your language`.
3. The inactive provider is always shown: second key card and second model table double the window.
4. Model tables (`Max tokens`, `Temperature`, `ignored`) sit in the middle of the page although almost
   nobody needs them. The Claude 5 hint is shown under the OpenAI table too. `AI provider` is both the
   card title and the field label.
5. Two save models: keys save instantly, everything else needs `Save settings`. That button is in the
   middle of the page, not visible while editing the top, and nothing shows unsaved changes.
6. The `Overlay` card holds unrelated things: `Split marker` (chat output) and
   `Log chat text (debugLogChat)` (troubleshooting, with a code name in the label). `(n/m)` and
   `0.82` are cryptic; `Width` / `Font size` have no units.
7. Every button is the same amber, so `Open logs folder` looks as important as `Save settings`.
   Folder buttons do not say what the folders are for.

## Goal

A first-time user, reading only the Settings window, can: pick a provider, add and test a key, pick a
language, and know the app is ready. Rarely used settings are out of the way but still one click away.

## What does NOT change

- `settings.json` keys and values (incl. `debugLogChat`, opacity stored as 0–1, both providers' models).
  No settings migration. Hidden fields keep their values.
- Key handling rules (Hard rule 2): key fields write-only, never pre-filled, cleared after save.
- Hotkey behavior, defaults, conflict marking.
- UI stays English.

---

## 1. New layout (top to bottom)

```
Roleplay Relay — Settings
RP chat helper

[Status banner]

AI service
  AI provider            [Anthropic (Claude) v]
  API key                ✓ Key saved      (or: ⚠ No key)
  [Paste your API key            ] [Save key] [Test key] [Clear key]
  Get a key at console.anthropic.com. Saved right away (no need to press Save settings).
  Stored in Windows Credential Manager and never shown here again.

Your language
  Your language          [Русский v]
  (existing hint + existing yellow prompt notes)

Hotkeys
  (as in change-hotkey-labels.md section 1)

Overlay
  Position               [Top right v]
  Width                  [420] px
  Font size              [16] px
  Opacity                [====|--] 82 %
  Auto-hide after        [12] seconds
  [Preview overlay]

Long replies
  Part marker            [(1/3) at the end v]
  Replies longer than {chat_limit} characters are split into parts. The marker shows which part this is.

Character and prompts
  [Open characters folder]  Your character (persona.md) and glossary.
  [Open prompts folder]     Instructions for each mode. For advanced users.

General
  [ ] Start with Windows

▸ Advanced                         (collapsed by default)
    Models — Anthropic (Claude)    (only the active provider's table)
    [Reset models]
    [ ] Save chat text in the log (for bug reports)
        Your chat messages will be written to app.log. Turn it off when you are done.
    [Open logs folder]

──────────────────────────────────────────────── (sticky footer)
Unsaved changes            [Discard] [Save settings]
```

### 1.1 Status banner
Computed in the frontend from data it already has (or one small backend command):
- All good → green: `Ready. Copy a chat line in the game and press Ctrl+Alt+T.` (show the
  **configured** combo).
- Otherwise amber, one line per problem, most important first:
  - `No {Provider} API key. Add it in AI service below.`
  - `{Label} hotkey is taken by another app. Pick another combination below.` (one per failed hotkey,
    using the new hotkey labels)
- Updates live after `Save key`, `Clear key`, a provider change and `Save settings`.

### 1.2 AI service card
- One card replaces `AI provider` + `Anthropic API key` + `OpenAI API key`. Card title `AI service`;
  field label stays `AI provider` (USER_GUIDE quotes it).
- Shows the key row **only for the selected provider** (as selected in the dropdown, even before
  `Save settings`). The other provider's key is untouched and reappears when that provider is selected.
- Status chip: `✓ Key saved` (green) / `⚠ No key` (amber), from `has_api_key(provider)`.
- Hint line per provider (plain text, not a link): Anthropic `Get a key at console.anthropic.com.`,
  OpenAI `Get a key at platform.openai.com.` Keep the existing Credential Manager sentence. Add
  `Saved right away (no need to press Save settings).`
- **`Test key`** (new): backend command `test_api_key(provider)` (allowed in the `settings` capability
  only). Sends one minimal request with that provider's `incoming` model (system: `Reply with OK.`,
  input `ping`, smallest `max_tokens` that works with the per-model request shape). Result inline:
  `✓ Key works` or the normal mapped error (`Invalid {Provider} API key`, `No credits left on your
  {Provider} account`, …). Never returns or logs the key. Disabled while no key is saved or a test is
  running. Uses the saved key, not the text field.
- Button style: `Save key` primary, `Test key` and `Clear key` secondary.

### 1.3 Overlay card
- Units in labels / suffixes: `px`, `seconds`.
- Opacity: slider 30–100 %, step 1, shown as `82 %`; stored as 0.30–1.00 (no settings change).
- Validate numbers inline (red text under the field, `Save settings` disabled while invalid):
  Width 300–900, Font size 12–28, Auto-hide 3–120. If the current code allows other ranges or a
  special value (e.g. 0 = never hide), keep that behavior and describe it in the hint instead.
- **`Preview overlay`** (new): shows the overlay for the auto-hide time with a sample entry
  (an English line + a translation in the selected user language, from a small sample table in
  `languages.rs`), using the values currently in the form (before save). Must keep all overlay
  rules: never activates, click-through. Sample text only, no request.

### 1.4 Long replies card
- `Split marker` → label `Part marker`. Options shown as examples: `(1/3) at the end`,
  `… at the end`, `None`. Stored values unchanged.
- Hint uses `{chat_limit}` from the active game profile (same source as the hotkey hints).

### 1.5 Character and prompts card
- Moves the two folder buttons out of `Folders`, as secondary buttons, each with a one-line hint
  (see layout). `Open logs folder` moves to Advanced.

### 1.6 Advanced (collapsed)
- Collapsed by default; remember open/closed per session only.
- Models table: only the selected provider. Title `Models — Anthropic (Claude)` / `Models — OpenAI`;
  drop `(active)`. The other provider's models stay in settings, untouched.
- Column labels: `Mode`, `Model`, `Max reply length (tokens)`, `Temperature`.
- Row labels: `Translate (incoming)`, `Direct`, `RP`, `Grammar` (match the reply window's mode names).
- Hint per provider: Anthropic — `Claude 5 models pick their own temperature, so the app does not send
  it.`; OpenAI — `Temperature: 0 = most exact, 1 = most creative. Leave empty for the model's default.`
- `Reset Anthropic models` / `Reset OpenAI models` → `Reset models` (resets the shown provider only).
- `Log chat text (debugLogChat)` → `Save chat text in the log (for bug reports)` + warning hint.

### 1.7 Saving
- Sticky footer, always visible at the bottom of the window: left `Unsaved changes` (only when the
  form differs from the stored settings), right `Discard` (secondary) and `Save settings` (primary,
  disabled when there is nothing to save or a field is invalid). After save: `Saved` for 2 s.
- Closing the Settings window with unsaved changes → small dialog
  `Save changes?` [`Save`] [`Discard`] [`Cancel`].
- `Start with Windows`: keep its current apply behavior; if it applies immediately, add the hint
  `Applies right away.`
- Visual hierarchy: amber filled buttons only for `Save settings` and `Save key`; all other buttons
  secondary (outline). Keep the existing dark theme and font stack.

---

## 2. CLAUDE.md edits

- **Windows** table, `settings` row: point to a new section **"Settings window"** with the layout,
  card rules and save behavior from section 1 of this file.
- **AI providers**: add `test_api_key(provider)` (settings capability only, minimal request, mapped
  errors, never logs the key).
- **Tauri hardening**: `settings` capability adds `test_api_key` and the game-profile read command.
  No opener URL scope change (key sites are plain text).
- **Windows / overlay**: add the preview rule (sample text, same no-activate + click-through).
- **Documentation**: USER_GUIDE section 8 follows the new card order; section 3 describes `Test key`.
- **Acceptance checklist**: add section 4 below as group "Settings UX" (together with the hotkey items).

## 3. Docs

- `docs/USER_GUIDE.md`:
  - section 3: one card `AI service`, key chip, `Test key` and its result messages;
  - section 8: rewrite in the new card order, with exact labels, hints, units and ranges; Advanced;
    the sticky footer and the close dialog;
  - section 10: add `Key works` and any new messages (banner lines, validation texts);
  - section 11: "Not sure the key works? Press `Test key`." Log section: the new label for chat logging;
  - replace Settings screenshots.
- Release notes (next version): "Settings: new layout. Status banner, `Test key`, only the active
  provider is shown, model settings moved to Advanced, overlay preview, sticky Save. No setting was
  reset; your keys, hotkeys and models are kept."

## 4. Acceptance

- [ ] Fresh install, no key: banner says `No Anthropic API key…`; after `Save key` + `Test key` →
      `✓ Key works` and banner turns green `Ready…` with the configured hotkey.
- [ ] Invalid key → `Test key` shows `Invalid Anthropic API key` next to the button. (The banner only
      checks that a key exists, so it does not change.)
- [ ] Selecting `OpenAI (beta)` (before save) shows only the OpenAI key row and, in Advanced, only the
      OpenAI table; switching back shows Anthropic again. Both keys and both model tables survive.
- [ ] Advanced is collapsed on open; `Save chat text in the log` maps to `debugLogChat`.
- [ ] Opacity slider shows `82 %` for a stored `0.82`; saving writes `0.82`.
- [ ] Out-of-range number → inline error, `Save settings` disabled.
- [ ] `Preview overlay` shows a sample in the selected language with the unsaved form values; the
      game window (or any focused fullscreen-windowed app) keeps focus; overlay is click-through.
- [ ] `Part marker` shows `(1/3) at the end` for stored `(n/m)`; hint shows the profile limit.
- [ ] Change any field → `Unsaved changes` appears; `Discard` restores stored values; closing the
      window asks `Save changes?`.
- [ ] Only `Save settings` and `Save key` are amber filled buttons.
- [ ] A hotkey taken by another app → banner line with the new hotkey label.
- [ ] `settings.json` from v0.1.0 loads unchanged; nothing is reset.
- [ ] Minimum window width: no horizontal scroll; footer does not cover the last card.
- [ ] `test_api_key` is not callable from `overlay` or `compose`; the key never reaches the WebView
      or the log.
- [ ] USER_GUIDE sections 3 and 8 match the window word for word.
- [ ] `npm test`, `cargo test`, `cargo clippy -D warnings` pass.
