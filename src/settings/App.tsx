import { useEffect, useState } from "react";
import {
  clearApiKey,
  getAutostart,
  getHotkeyStatus,
  getPromptLanguageNotes,
  getSettings,
  hasApiKey,
  listLanguages,
  openDataFolder,
  saveSettings,
  setApiKey,
  setAutostart,
} from "../shared/api";
import type { HotkeyStatus, LanguageOption, Mode, ModeConfig, Settings } from "../shared/types";

function errorText(e: unknown): string {
  return typeof e === "string" ? e : "Something went wrong";
}

const HOTKEY_ROWS: { id: string; label: string }[] = [
  { id: "translate", label: "Translate clipboard" },
  { id: "compose", label: "Compose reply" },
  { id: "quick", label: "Quick clipboard mode" },
  { id: "nextPart", label: "Copy next part" },
  { id: "overlayToggle", label: "Show/Hide overlay" },
];
const HOTKEY_DEFAULTS: Record<string, string> = {
  translate: "Ctrl+Alt+T",
  compose: "Ctrl+Alt+R",
  quick: "Ctrl+Alt+E",
  nextPart: "Ctrl+Alt+N",
  overlayToggle: "Ctrl+Alt+O",
};
const MODE_ROWS: { id: Mode; label: string }[] = [
  { id: "incoming", label: "Incoming" },
  { id: "direct", label: "Direct" },
  { id: "rp", label: "RP" },
  { id: "grammar", label: "Grammar" },
];
const MODE_DEFAULTS: Record<Mode, ModeConfig> = {
  incoming: { model: "claude-haiku-5-5", maxTokens: 800, temperature: 0.2 },
  direct: { model: "claude-sonnet-5-5", maxTokens: 800, temperature: 0.3 },
  rp: { model: "claude-sonnet-5-5", maxTokens: 800, temperature: 0.7 },
  grammar: { model: "claude-haiku-5-5", maxTokens: 400, temperature: 0 },
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

export default function App() {
  const [keySet, setKeySet] = useState<boolean | null>(null);
  const [keyInput, setKeyInput] = useState("");
  const [keyMsg, setKeyMsg] = useState<string | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [saveMsg, setSaveMsg] = useState<string | null>(null);
  const [hkStatus, setHkStatus] = useState<HotkeyStatus[]>([]);
  const [autostart, setAutostartState] = useState<boolean | null>(null);
  const [autostartMsg, setAutostartMsg] = useState<string | null>(null);
  const [languages, setLanguages] = useState<LanguageOption[]>([]);
  const [oldPrompts, setOldPrompts] = useState<string[]>([]);

  useEffect(() => {
    hasApiKey().then(setKeySet).catch(() => setKeySet(false));
    getSettings().then(setSettings).catch((e) => setSaveMsg(errorText(e)));
    getHotkeyStatus().then(setHkStatus).catch(() => {});
    getAutostart().then(setAutostartState).catch(() => setAutostartState(false));
    listLanguages().then(setLanguages).catch(() => {});
    getPromptLanguageNotes().then(setOldPrompts).catch(() => {});
  }, []);

  async function onSaveKey() {
    const key = keyInput.trim();
    if (!key) return;
    setKeyInput("");
    try {
      await setApiKey(key);
      setKeySet(true);
      setKeyMsg("Key saved");
    } catch (e) {
      setKeyMsg(errorText(e));
    }
  }

  async function onClearKey() {
    try {
      await clearApiKey();
      setKeySet(false);
      setKeyMsg("Key removed");
    } catch (e) {
      setKeyMsg(errorText(e));
    }
  }

  async function onSaveSettings() {
    if (!settings) return;
    try {
      const status = await saveSettings(settings);
      setHkStatus(status);
      // Show what the backend actually stored (it clamps and repairs values).
      setSettings(await getSettings());
      const bad = status.filter((s) => !s.ok).length;
      setSaveMsg(bad ? "Settings saved — some hotkeys could not be registered" : "Settings saved");
    } catch (e) {
      setSaveMsg(errorText(e));
    }
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
  const setHotkey = (id: string, value: string) =>
    settings && update({ hotkeys: { ...settings.hotkeys, [id]: value } });
  const setMode = (id: Mode, patch: Partial<ModeConfig>) =>
    settings && update({ modes: { ...settings.modes, [id]: { ...settings.modes[id], ...patch } } });

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

  return (
    <main>
      <h1>WoW Chat Helper — Settings</h1>

      <section>
        <h2>Anthropic API key</h2>
        <p className="status">
          {keySet === null ? "Checking…" : keySet ? "API key is set" : "No API key set"}
        </p>
        <div className="row">
          <input
            type="password"
            value={keyInput}
            onChange={(e) => setKeyInput(e.target.value)}
            placeholder="Paste your API key"
            autoComplete="off"
            spellCheck={false}
            aria-label="API key"
          />
          <button onClick={onSaveKey} disabled={!keyInput.trim()}>
            Save key
          </button>
          <button onClick={onClearKey} disabled={!keySet}>
            Clear key
          </button>
        </div>
        {keyMsg && <p className="msg">{keyMsg}</p>}
        <p className="hint">
          The key is stored in Windows Credential Manager. It is never shown here again.
        </p>
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
        {autostartMsg && <p className="msg">{autostartMsg}</p>}
      </section>

      {settings && (
        <>
          <section>
            <h2>Language</h2>
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
            {HOTKEY_ROWS.map((r) => {
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
                    <button
                      className="secondary"
                      onClick={() => setHotkey(r.id, HOTKEY_DEFAULTS[r.id])}
                    >
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
                </div>
              );
            })}
            <p className="hint">
              Click a field and press the new combination. Use at least one of Ctrl, Alt, Shift.
              Changes apply when you press Save settings.
            </p>
          </section>

          <section>
            <h2>Models</h2>
            <div className="modes">
              <span className="head">Mode</span>
              <span className="head">Model</span>
              <span className="head">Max tokens</span>
              <span className="head">Temperature</span>
              {MODE_ROWS.map((r) => (
                <ModeRow
                  key={r.id}
                  label={r.label}
                  cfg={settings.modes[r.id]}
                  onChange={(p) => setMode(r.id, p)}
                />
              ))}
            </div>
            <div className="row">
              <button
                className="secondary"
                onClick={() => update({ modes: structuredClone(MODE_DEFAULTS) })}
              >
                Reset models
              </button>
            </div>
            <p className="hint">Leave temperature empty to let the model use its own default.</p>
          </section>

          <section>
            <h2>Overlay</h2>
            <label>
              Position
              <select
                value={settings.overlay.position}
                onChange={(e) =>
                  update({
                    overlay: {
                      ...settings.overlay,
                      position: e.target.value as Settings["overlay"]["position"],
                    },
                  })
                }
              >
                <option value="top-right">Top right</option>
                <option value="top-left">Top left</option>
                <option value="bottom-right">Bottom right</option>
                <option value="bottom-left">Bottom left</option>
              </select>
            </label>
            <label>
              Width
              <input
                type="number"
                min={240}
                max={900}
                value={settings.overlay.width}
                onChange={(e) =>
                  update({ overlay: { ...settings.overlay, width: Number(e.target.value) } })
                }
              />
            </label>
            <label>
              Font size
              <input
                type="number"
                min={12}
                max={32}
                value={settings.overlay.fontSize}
                onChange={(e) =>
                  update({ overlay: { ...settings.overlay, fontSize: Number(e.target.value) } })
                }
              />
            </label>
            <label>
              Opacity
              <input
                type="number"
                min={0.2}
                max={1}
                step={0.05}
                value={settings.overlay.opacity}
                onChange={(e) =>
                  update({ overlay: { ...settings.overlay, opacity: Number(e.target.value) } })
                }
              />
            </label>
            <label>
              Auto-hide (seconds)
              <input
                type="number"
                min={2}
                max={120}
                value={settings.overlay.autoHideSeconds}
                onChange={(e) =>
                  update({
                    overlay: { ...settings.overlay, autoHideSeconds: Number(e.target.value) },
                  })
                }
              />
            </label>
            <label>
              Split marker
              <select
                value={settings.splitMarker}
                onChange={(e) =>
                  update({ splitMarker: e.target.value as Settings["splitMarker"] })
                }
              >
                <option value="(n/m)">(n/m)</option>
                <option value="…">…</option>
                <option value="none">none</option>
              </select>
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={settings.debugLogChat}
                onChange={(e) => update({ debugLogChat: e.target.checked })}
              />
              Log chat text (debugLogChat)
            </label>
          </section>

          <div className="row">
            <button onClick={onSaveSettings}>Save settings</button>
            {saveMsg && <span className="msg">{saveMsg}</span>}
          </div>
        </>
      )}

      <section style={{ marginTop: 16 }}>
        <h2>Folders</h2>
        <div className="row">
          <button onClick={() => openDataFolder("prompts")}>Open prompts folder</button>
          <button onClick={() => openDataFolder("characters")}>Open characters folder</button>
          <button onClick={() => openDataFolder("logs")}>Open logs folder</button>
        </div>
      </section>
    </main>
  );
}

function ModeRow({
  label,
  cfg,
  onChange,
}: {
  label: string;
  cfg: ModeConfig;
  onChange: (p: Partial<ModeConfig>) => void;
}) {
  return (
    <>
      <span>{label}</span>
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
        aria-label={`${label} max tokens`}
        onChange={(e) => onChange({ maxTokens: Number(e.target.value) })}
      />
      <input
        type="number"
        min={0}
        max={1}
        step={0.1}
        value={cfg.temperature ?? ""}
        aria-label={`${label} temperature`}
        onChange={(e) =>
          onChange({ temperature: e.target.value === "" ? null : Number(e.target.value) })
        }
      />
    </>
  );
}
