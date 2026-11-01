import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  clearApiKey,
  closeSettings,
  getAutostart,
  getGameProfile,
  getHotkeyStatus,
  getLastMode,
  getPromptLanguageNotes,
  getSettings,
  hasApiKey,
  listLanguages,
  openDataFolder,
  previewOverlay,
  saveSettings,
  setApiKey,
  setAutostart,
  testApiKey,
} from "../shared/api";
import type {
  GameInfo,
  HotkeyStatus,
  LanguageOption,
  Mode,
  ModeConfig,
  OutgoingMode,
  Provider,
  Settings,
} from "../shared/types";

function errorText(e: unknown): string {
  return typeof e === "string" ? e : "Something went wrong";
}

const HOTKEY_GROUPS: { title: string; rows: { id: string; label: string }[] }[] = [
  { title: "Reading chat", rows: [{ id: "translate", label: "Translate copied line" }] },
  {
    title: "Writing replies",
    rows: [
      { id: "compose", label: "Open reply window" },
      { id: "quick", label: "Convert copied text" },
      { id: "nextPart", label: "Copy next part" },
    ],
  },
  { title: "Overlay", rows: [{ id: "overlayToggle", label: "Show/Hide overlay" }] },
];
const HOTKEY_ROWS = HOTKEY_GROUPS.flatMap((g) => g.rows);
const HOTKEY_DEFAULTS: Record<string, string> = {
  translate: "Ctrl+Alt+T",
  compose: "Ctrl+Alt+R",
  quick: "Ctrl+Alt+E",
  nextPart: "Ctrl+Alt+N",
  overlayToggle: "Ctrl+Alt+O",
};
const MODE_NAMES: Record<OutgoingMode, string> = { direct: "Direct", rp: "RP", grammar: "Grammar" };
const MODE_ROWS: { id: Mode; label: string }[] = [
  { id: "incoming", label: "Translate (incoming)" },
  { id: "direct", label: "Direct" },
  { id: "rp", label: "RP" },
  { id: "grammar", label: "Grammar" },
];
const PROVIDERS: {
  id: Provider;
  label: string;
  name: string;
  keyHint: string;
  modelsTitle: string;
  modelsHint: string;
}[] = [
  {
    id: "anthropic",
    label: "Anthropic (Claude)",
    name: "Anthropic",
    keyHint: "Get a key at console.anthropic.com.",
    modelsTitle: "Models — Anthropic (Claude)",
    modelsHint:
      "Claude 5 models only accept their default temperature, so the app doesn't send it. To change how replies sound, edit rp.md or persona.md (Open prompts folder / Open characters folder).",
  },
  {
    id: "openai",
    label: "OpenAI (beta)",
    name: "OpenAI",
    keyHint: "Get a key at platform.openai.com.",
    modelsTitle: "Models — OpenAI",
    modelsHint:
      "Temperature: 0 = most exact, 1 = most creative. Leave empty for the model's default.",
  },
];
const MODE_DEFAULTS: Record<Provider, Record<Mode, ModeConfig>> = {
  anthropic: {
    incoming: { model: "claude-haiku-5-5", maxTokens: 800, temperature: 0.2 },
    direct: { model: "claude-sonnet-5-5", maxTokens: 800, temperature: 0.3 },
    rp: { model: "claude-sonnet-5-5", maxTokens: 800, temperature: 0.7 },
    grammar: { model: "claude-haiku-5-5", maxTokens: 400, temperature: 0 },
  },
  openai: {
    incoming: { model: "gpt-6-luna", maxTokens: 800, temperature: 0.2 },
    direct: { model: "gpt-6.1-terra", maxTokens: 800, temperature: 0.3 },
    rp: { model: "gpt-6.1-terra", maxTokens: 800, temperature: 0.7 },
    grammar: { model: "gpt-6-luna", maxTokens: 400, temperature: 0 },
  },
};
// The ranges the backend applies on save (settings.rs `sanitized`); the form flags a value
// before the backend would silently change it.
const RANGES = {
  width: { min: 240, max: 900 },
  fontSize: { min: 12, max: 32 },
  autoHideSeconds: { min: 2, max: 120 },
};

/** Turns a key press into the combo text the backend parses, or null while only modifiers are down. */
function comboFromEvent(e: React.KeyboardEvent): string | null {
  const c = e.code;
  let key: string | null = null;
  if (/^Key[A-Z]$/.test(c)) key = c.slice(3);
  else if (/^Digit\d$/.test(c)) key = c.slice(5);
  else if (/^F([1-9]|1\d|2[0-4])$/.test(c)) key = c;
  else if (["Space", "Insert", "Delete", "Home", "End", "PageUp", "PageDown"].includes(c)) key = c;
  else if (/^Arrow(Up|Down|Left|Right)$/.test(c)) key = c;
  if (!key) return null;
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"]
    .filter(Boolean) as string[];
  return [...mods, key].join("+");
}

/** Opacity is stored as 0–1 with two decimals; compare it as whole percent (f32 round trips). */
const snapshot = (s: Settings | null) =>
  JSON.stringify(
    s && { ...s, overlay: { ...s.overlay, opacity: Math.round(s.overlay.opacity * 100) } },
  );

type KeyResult = { ok: boolean; text: string } | null;

export default function App() {
  const [keySet, setKeySet] = useState<Record<Provider, boolean | null>>({
    anthropic: null,
    openai: null,
  });
  const [keyInput, setKeyInput] = useState<Record<Provider, string>>({ anthropic: "", openai: "" });
  const [keyMsg, setKeyMsg] = useState<Record<Provider, string | null>>({
    anthropic: null,
    openai: null,
  });
  const [testing, setTesting] = useState<Record<Provider, boolean>>({
    anthropic: false,
    openai: false,
  });
  const [testResult, setTestResult] = useState<Record<Provider, KeyResult>>({
    anthropic: null,
    openai: null,
  });
  const [settings, setSettings] = useState<Settings | null>(null);
  /** What is stored right now; the form has unsaved changes when it differs. */
  const [stored, setStored] = useState<Settings | null>(null);
  const [saveMsg, setSaveMsg] = useState<string | null>(null);
  const [hkStatus, setHkStatus] = useState<HotkeyStatus[]>([]);
  const [autostart, setAutostartState] = useState<boolean | null>(null);
  const [autostartMsg, setAutostartMsg] = useState<string | null>(null);
  const [languages, setLanguages] = useState<LanguageOption[]>([]);
  const [oldPrompts, setOldPrompts] = useState<string[]>([]);
  const [game, setGame] = useState<GameInfo | null>(null);
  const [lastMode, setLastMode] = useState<OutgoingMode>("direct");
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [closing, setClosing] = useState(false);

  const refreshKey = (p: Provider) =>
    hasApiKey(p)
      .then((v) => setKeySet((s) => ({ ...s, [p]: v })))
      .catch(() => setKeySet((s) => ({ ...s, [p]: false })));

  const dirty = settings !== null && snapshot(settings) !== snapshot(stored);

  useEffect(() => {
    for (const p of PROVIDERS) refreshKey(p.id);
    getSettings()
      .then((s) => {
        setSettings(s);
        setStored(s);
      })
      .catch((e) => setSaveMsg(errorText(e)));
    getHotkeyStatus().then(setHkStatus).catch(() => {});
    getAutostart().then(setAutostartState).catch(() => setAutostartState(false));
    listLanguages().then(setLanguages).catch(() => {});
    getPromptLanguageNotes().then(setOldPrompts).catch(() => {});
    getGameProfile().then(setGame).catch(() => {});
    getLastMode().then(setLastMode).catch(() => {});
  }, []);

  // The reply window may have changed the mode while Settings was open or hidden.
  useEffect(() => {
    const refresh = () => {
      getLastMode().then(setLastMode).catch(() => {});
      getHotkeyStatus().then(setHkStatus).catch(() => {});
      getPromptLanguageNotes().then(setOldPrompts).catch(() => {});
      for (const p of PROVIDERS) refreshKey(p.id);
    };
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, []);

  // Rust asks the form before the window closes: ask the user when there is something to lose.
  const dirtyRef = useRef(false);
  dirtyRef.current = dirty;
  useEffect(() => {
    const sub = listen("settings://close-requested", () => {
      if (dirtyRef.current) setClosing(true);
      else closeSettings();
    });
    return () => {
      sub.then((fn) => fn());
    };
  }, []);

  const setKeyInputFor = (p: Provider, v: string) => setKeyInput((s) => ({ ...s, [p]: v }));
  const setKeyMsgFor = (p: Provider, v: string | null) => setKeyMsg((s) => ({ ...s, [p]: v }));
  const setKeySetFor = (p: Provider, v: boolean) => setKeySet((s) => ({ ...s, [p]: v }));
  const setTestFor = (p: Provider, v: KeyResult) => setTestResult((s) => ({ ...s, [p]: v }));

  async function onSaveKey(p: Provider) {
    const key = keyInput[p].trim();
    if (!key) return;
    // Write-only field: clear it right away, whatever the outcome.
    setKeyInputFor(p, "");
    setTestFor(p, null);
    try {
      await setApiKey(p, key);
      setKeySetFor(p, true);
      setKeyMsgFor(p, "Key saved");
    } catch (e) {
      setKeyMsgFor(p, errorText(e));
    }
  }

  async function onClearKey(p: Provider) {
    setTestFor(p, null);
    try {
      await clearApiKey(p);
      setKeySetFor(p, false);
      setKeyMsgFor(p, "Key removed");
    } catch (e) {
      setKeyMsgFor(p, errorText(e));
    }
  }

  async function onTestKey(p: Provider) {
    setTesting((s) => ({ ...s, [p]: true }));
    setTestFor(p, null);
    try {
      await testApiKey(p);
      setTestFor(p, { ok: true, text: "Key works" });
    } catch (e) {
      setTestFor(p, { ok: false, text: errorText(e) });
    } finally {
      setTesting((s) => ({ ...s, [p]: false }));
    }
  }

  /** Returns true when the settings were stored. */
  async function onSaveSettings(): Promise<boolean> {
    if (!settings || invalid) return false;
    try {
      const status = await saveSettings(settings);
      setHkStatus(status);
      // Show what the backend actually stored (it clamps and repairs values).
      const fresh = await getSettings();
      setSettings(fresh);
      setStored(fresh);
      const bad = status.filter((s) => !s.ok).length;
      setSaveMsg(bad ? "Saved — some hotkeys could not be registered" : "Saved");
      setTimeout(() => setSaveMsg(null), 2000);
      return true;
    } catch (e) {
      setSaveMsg(errorText(e));
      return false;
    }
  }

  function onDiscard() {
    setSettings(stored);
    setSaveMsg(null);
  }

  async function onAutostart(enabled: boolean) {
    try {
      await setAutostart(enabled);
      setAutostartState(enabled);
      setAutostartMsg(null);
    } catch (e) {
      setAutostartMsg(errorText(e));
    }
  }

  const update = (patch: Partial<Settings>) =>
    setSettings((s) => (s ? { ...s, ...patch } : s));
  const updateOverlay = (patch: Partial<Settings["overlay"]>) =>
    setSettings((s) => (s ? { ...s, overlay: { ...s.overlay, ...patch } } : s));
  const setHotkey = (id: string, value: string) =>
    settings && update({ hotkeys: { ...settings.hotkeys, [id]: value } });
  const setMode = (provider: Provider, id: Mode, patch: Partial<ModeConfig>) =>
    settings &&
    update({
      models: {
        ...settings.models,
        [provider]: {
          ...settings.models[provider],
          [id]: { ...settings.models[provider][id], ...patch },
        },
      },
    });

  /** Problem for a row: the live registration result, or a duplicate in the unsaved form. */
  function hotkeyProblem(id: string): string | null {
    if (!settings) return null;
    const mine = settings.hotkeys[id];
    const dup = HOTKEY_ROWS.some(
      (r) => r.id !== id && settings.hotkeys[r.id]?.toLowerCase() === mine?.toLowerCase(),
    );
    if (dup) return "Same as another hotkey";
    const st = hkStatus.find((s) => s.action === id);
    return st && !st.ok ? st.error : null;
  }

  const rangeError = (label: string, value: number, r: { min: number; max: number }) =>
    Number.isInteger(value) && value >= r.min && value <= r.max
      ? null
      : `${label} must be a whole number from ${r.min} to ${r.max}.`;
  const errors = settings && {
    width: rangeError("Width", settings.overlay.width, RANGES.width),
    fontSize: rangeError("Font size", settings.overlay.fontSize, RANGES.fontSize),
    autoHideSeconds: rangeError(
      "Auto-hide",
      settings.overlay.autoHideSeconds,
      RANGES.autoHideSeconds,
    ),
  };
  const invalid = !!errors && Object.values(errors).some(Boolean);

  const provider = PROVIDERS.find((p) => p.id === settings?.provider) ?? PROVIDERS[0];
  const language = languages.find((l) => l.code === settings?.userLanguage);

  // ---- status banner ----
  const problems: string[] = [];
  if (settings && keySet[provider.id] === false) {
    problems.push(`No ${provider.name} API key. Add it in AI service below.`);
  }
  for (const st of hkStatus) {
    if (st.ok) continue;
    const label = HOTKEY_ROWS.find((r) => r.id === st.action)?.label ?? st.action;
    problems.push(
      st.error === "Used by another app"
        ? `${label} hotkey is taken by another app. Pick another combination below.`
        : `${label} hotkey: ${st.error ?? "not registered"}. Pick another combination below.`,
    );
  }
  const ready = !!stored && keySet[provider.id] === true && problems.length === 0;

  return (
    <main>
      <h1>Roleplay Relay — Settings</h1>
      <p className="status">RP chat helper</p>

      {settings && (
        <>
          {ready ? (
            <p className="banner ok">
              Ready. Copy a chat line in the game and press {stored?.hotkeys.translate}.
            </p>
          ) : (
            problems.length > 0 && (
              <div className="banner warn-banner">
                {problems.map((t) => (
                  <p key={t}>{t}</p>
                ))}
              </div>
            )
          )}

          <section>
            <h2>AI service</h2>
            <label>
              AI provider
              <select
                value={settings.provider}
                onChange={(e) => update({ provider: e.target.value as Provider })}
              >
                {PROVIDERS.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.label}
                  </option>
                ))}
              </select>
            </label>
            <div className="keyline">
              <span>API key</span>
              {keySet[provider.id] === null ? (
                <span className="chip">Checking…</span>
              ) : keySet[provider.id] ? (
                <span className="chip good">✓ Key saved</span>
              ) : (
                <span className="chip warn">⚠ No key</span>
              )}
            </div>
            <div className="row">
              <input
                type="password"
                value={keyInput[provider.id]}
                onChange={(e) => setKeyInputFor(provider.id, e.target.value)}
                placeholder="Paste your API key"
                autoComplete="off"
                spellCheck={false}
                aria-label={`${provider.name} API key`}
              />
              <button
                className="primary"
                onClick={() => onSaveKey(provider.id)}
                disabled={!keyInput[provider.id].trim()}
              >
                Save key
              </button>
              <button
                onClick={() => onTestKey(provider.id)}
                disabled={!keySet[provider.id] || testing[provider.id]}
              >
                {testing[provider.id] ? "Testing…" : "Test key"}
              </button>
              <button onClick={() => onClearKey(provider.id)} disabled={!keySet[provider.id]}>
                Clear key
              </button>
            </div>
            {testResult[provider.id] && (
              <p className={testResult[provider.id]!.ok ? "msg good-text" : "err"}>
                {testResult[provider.id]!.ok ? "✓ " : ""}
                {testResult[provider.id]!.text}
              </p>
            )}
            {keyMsg[provider.id] && <p className="msg">{keyMsg[provider.id]}</p>}
            <p className="hint">
              {provider.keyHint} Saved right away (no need to press Save settings).
            </p>
            <p className="hint">
              Stored in Windows Credential Manager and never shown here again.
            </p>
          </section>

          <section>
            <h2>Your language</h2>
            <label>
              Your language
              <select
                className="lang-select"
                value={settings.userLanguage}
                onChange={(e) => update({ userLanguage: e.target.value })}
              >
                {languages.map((l) => (
                  <option key={l.code} value={l.code}>
                    {l.label}
                  </option>
                ))}
              </select>
            </label>
            <p className="hint">
              Translations and back-translations are written in this language. Applies from the
              next request after you save. The app itself stays in English.
            </p>
            {settings.userLanguage !== "ru" &&
              oldPrompts.map((f) => (
                <p className="warn" key={f}>
                  {f} doesn't use your language setting. Delete it and restart to get the default.
                </p>
              ))}
          </section>

          <section>
            <h2>Hotkeys</h2>
            {HOTKEY_GROUPS.map((g) => (
              <div key={g.title}>
                <h3>{g.title}</h3>
                {g.rows.map((r) => {
                  const problem = hotkeyProblem(r.id);
                  return (
                    <div key={r.id}>
                      <div className="hk">
                        <span>{r.label}</span>
                        <input
                          readOnly
                          className={problem ? "bad" : ""}
                          value={settings.hotkeys[r.id] ?? ""}
                          aria-label={`${r.label} hotkey`}
                          title="Click, then press the new combination"
                          onKeyDown={(e) => {
                            if (e.key === "Tab") return;
                            e.preventDefault();
                            const combo = comboFromEvent(e);
                            if (combo) setHotkey(r.id, combo);
                          }}
                        />
                        <button onClick={() => setHotkey(r.id, HOTKEY_DEFAULTS[r.id])}>
                          Reset
                        </button>
                        <span>{problem ? "✗" : ""}</span>
                      </div>
                      {problem && (
                        <div className="hk">
                          <span />
                          <span className="err">{problem}</span>
                        </div>
                      )}
                      <p className="hk-hint">
                        {hotkeyHint(r.id, language?.englishName ?? "your language", lastMode, game)}
                      </p>
                    </div>
                  );
                })}
              </div>
            ))}
            <p className="hint">
              Click a field and press the new combination. Use at least one of Ctrl, Alt, Shift.
              Changes apply when you press Save settings.
            </p>
          </section>

          <section>
            <h2>Overlay</h2>
            <label>
              Position
              <select
                value={settings.overlay.position}
                onChange={(e) =>
                  updateOverlay({ position: e.target.value as Settings["overlay"]["position"] })
                }
              >
                <option value="top-right">Top right</option>
                <option value="top-left">Top left</option>
                <option value="bottom-right">Bottom right</option>
                <option value="bottom-left">Bottom left</option>
              </select>
            </label>
            <NumberField
              label="Width"
              suffix="px"
              value={settings.overlay.width}
              error={errors?.width ?? null}
              onChange={(v) => updateOverlay({ width: v })}
            />
            <NumberField
              label="Font size"
              suffix="px"
              value={settings.overlay.fontSize}
              error={errors?.fontSize ?? null}
              onChange={(v) => updateOverlay({ fontSize: v })}
            />
            <label>
              Opacity
              <span className="slider">
                <input
                  type="range"
                  min={20}
                  max={100}
                  step={1}
                  value={Math.round(settings.overlay.opacity * 100)}
                  aria-label="Opacity"
                  onChange={(e) => updateOverlay({ opacity: Number(e.target.value) / 100 })}
                />
                <span className="value">{Math.round(settings.overlay.opacity * 100)} %</span>
              </span>
            </label>
            <NumberField
              label="Auto-hide after"
              suffix="seconds"
              value={settings.overlay.autoHideSeconds}
              error={errors?.autoHideSeconds ?? null}
              onChange={(v) => updateOverlay({ autoHideSeconds: v })}
            />
            <div className="row">
              <button
                onClick={() => previewOverlay(settings.overlay, settings.userLanguage)}
                disabled={invalid}
              >
                Preview overlay
              </button>
            </div>
            <p className="hint">Shows a sample with the values above, even before you save them.</p>
          </section>

          <section>
            <h2>Long replies</h2>
            <label>
              Part marker
              <select
                value={settings.splitMarker}
                onChange={(e) =>
                  update({ splitMarker: e.target.value as Settings["splitMarker"] })
                }
              >
                <option value="(n/m)">(1/3) at the end</option>
                <option value="…">… at the end</option>
                <option value="none">None</option>
              </select>
            </label>
            <p className="hint">
              Replies longer than {game ? game.chatLimit : "…"} characters are split into parts.
              The marker shows which part this is.
            </p>
          </section>

          <section>
            <h2>Character and prompts</h2>
            <div className="folder">
              <button onClick={() => openDataFolder("characters")}>Open characters folder</button>
              <span className="hint">Your character (persona.md) and glossary.</span>
            </div>
            <div className="folder">
              <button onClick={() => openDataFolder("prompts")}>Open prompts folder</button>
              <span className="hint">Instructions for each mode. For advanced users.</span>
            </div>
          </section>

          <section>
            <h2>General</h2>
            <label className="check">
              <input
                type="checkbox"
                checked={autostart ?? false}
                disabled={autostart === null}
                onChange={(e) => onAutostart(e.target.checked)}
              />
              Start with Windows
            </label>
            <p className="hint">Applies right away.</p>
            {autostartMsg && <p className="msg">{autostartMsg}</p>}
          </section>

          <section>
            <button
              className="disclosure"
              aria-expanded={advancedOpen}
              onClick={() => setAdvancedOpen((o) => !o)}
            >
              {advancedOpen ? "▾" : "▸"} Advanced
            </button>
            {advancedOpen && (
              <div>
                <h3>{provider.modelsTitle}</h3>
                <div className="modes">
                  <span className="head">Mode</span>
                  <span className="head">Model</span>
                  <span className="head">Max reply length (tokens)</span>
                  <span className="head">Temperature</span>
                  {MODE_ROWS.map((r) => (
                    <ModeRow
                      key={r.id}
                      label={`${provider.name} ${r.label}`}
                      title={r.label}
                      cfg={settings.models[provider.id][r.id]}
                      provider={provider.id}
                      onChange={(patch) => setMode(provider.id, r.id, patch)}
                    />
                  ))}
                </div>
                <p className="hint">{provider.modelsHint}</p>
                <div className="row">
                  <button
                    onClick={() =>
                      update({
                        models: {
                          ...settings.models,
                          [provider.id]: structuredClone(MODE_DEFAULTS[provider.id]),
                        },
                      })
                    }
                  >
                    Reset models
                  </button>
                </div>

                <label className="check log-check">
                  <input
                    type="checkbox"
                    checked={settings.debugLogChat}
                    onChange={(e) => update({ debugLogChat: e.target.checked })}
                  />
                  Save chat text in the log (for bug reports)
                </label>
                <p className="hint">
                  Your chat messages will be written to app.log. Turn it off when you are done.
                </p>
                <div className="row">
                  <button onClick={() => openDataFolder("logs")}>Open logs folder</button>
                </div>
              </div>
            )}
          </section>
        </>
      )}

      <footer className="footer">
        <span className={dirty ? "unsaved" : "saved"}>
          {dirty ? "Unsaved changes" : (saveMsg ?? "")}
        </span>
        <span className="footer-buttons">
          <button onClick={onDiscard} disabled={!dirty}>
            Discard
          </button>
          <button className="primary" onClick={onSaveSettings} disabled={!dirty || invalid}>
            Save settings
          </button>
        </span>
      </footer>

      {closing && (
        <div className="modal-back">
          <div className="modal" role="dialog" aria-label="Save changes?">
            <h2>Save changes?</h2>
            <div className="row">
              <button
                className="primary"
                onClick={async () => {
                  const saved = await onSaveSettings();
                  setClosing(false);
                  if (saved) closeSettings();
                }}
              >
                Save
              </button>
              <button
                onClick={() => {
                  onDiscard();
                  setClosing(false);
                  closeSettings();
                }}
              >
                Discard
              </button>
              <button onClick={() => setClosing(false)}>Cancel</button>
            </div>
          </div>
        </div>
      )}
    </main>
  );
}

/** The hint under a hotkey row; the language, mode and chat limit are live values. */
function hotkeyHint(
  id: string,
  language: string,
  mode: OutgoingMode,
  game: GameInfo | null,
): string {
  switch (id) {
    case "translate":
      return `Copy someone's chat line first. Shows the ${language} translation in the overlay. Your clipboard stays the same.`;
    case "compose":
      return "Type in any language. The English result goes to your clipboard.";
    case "quick":
      return `Copy your own text first. The English result replaces your clipboard. Uses ${MODE_NAMES[mode]} mode, the last mode you picked in the reply window.`;
    case "nextPart":
      return `For replies longer than ${game ? game.chatLimit : "…"} characters. Copies the next part.`;
    default:
      return "Shows your last 10 messages and translations.";
  }
}

function NumberField({
  label,
  suffix,
  value,
  error,
  onChange,
}: {
  label: string;
  suffix: string;
  value: number;
  error: string | null;
  onChange: (v: number) => void;
}) {
  return (
    <div>
      <label>
        {label}
        <span className="num">
          <input
            type="number"
            className={error ? "bad" : ""}
            value={Number.isNaN(value) ? "" : value}
            aria-label={label}
            onChange={(e) => onChange(e.target.value === "" ? NaN : Number(e.target.value))}
          />
          <span className="unit">{suffix}</span>
        </span>
      </label>
      {error && <p className="err">{error}</p>}
    </div>
  );
}

/** Mirrors `claude5_shape` in llm/anthropic.rs: Claude 5 models never get the mode's temperature. */
function ignoresTemperature(provider: Provider, model: string): boolean {
  return provider === "anthropic" && /^claude-[^-]+-5/.test(model.trim());
}

function ModeRow({
  label,
  title,
  cfg,
  provider,
  onChange,
}: {
  label: string;
  title: string;
  cfg: ModeConfig;
  provider: Provider;
  onChange: (p: Partial<ModeConfig>) => void;
}) {
  const tempIgnored = ignoresTemperature(provider, cfg.model);
  return (
    <>
      <span>{title}</span>
      <input
        value={cfg.model}
        spellCheck={false}
        aria-label={`${label} model`}
        onChange={(e) => onChange({ model: e.target.value })}
      />
      <input
        type="number"
        min={50}
        max={4000}
        value={cfg.maxTokens}
        aria-label={`${label} max reply length`}
        onChange={(e) => onChange({ maxTokens: Number(e.target.value) })}
      />
      <input
        type="number"
        min={0}
        max={1}
        step={0.1}
        value={tempIgnored ? "" : (cfg.temperature ?? "")}
        placeholder={tempIgnored ? "ignored" : undefined}
        disabled={tempIgnored}
        aria-label={`${label} temperature`}
        onChange={(e) =>
          onChange({ temperature: e.target.value === "" ? null : Number(e.target.value) })
        }
      />
    </>
  );
}
