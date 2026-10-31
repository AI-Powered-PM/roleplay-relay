import { useEffect, useState } from "react";
import {
  clearApiKey,
  getSettings,
  hasApiKey,
  openDataFolder,
  saveSettings,
  setApiKey,
} from "../shared/api";
import type { Settings } from "../shared/types";

function errorText(e: unknown): string {
  return typeof e === "string" ? e : "Something went wrong";
}

export default function App() {
  const [keySet, setKeySet] = useState<boolean | null>(null);
  const [keyInput, setKeyInput] = useState("");
  const [keyMsg, setKeyMsg] = useState<string | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [saveMsg, setSaveMsg] = useState<string | null>(null);

  useEffect(() => {
    hasApiKey().then(setKeySet).catch(() => setKeySet(false));
    getSettings().then(setSettings).catch((e) => setSaveMsg(errorText(e)));
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
      await saveSettings(settings);
      setSaveMsg("Settings saved");
    } catch (e) {
      setSaveMsg(errorText(e));
    }
  }

  const update = (patch: Partial<Settings>) =>
    setSettings((s) => (s ? { ...s, ...patch } : s));

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

      {settings && (
        <section>
          <h2>Overlay</h2>
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
              onChange={(e) => update({ splitMarker: e.target.value as Settings["splitMarker"] })}
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
          <div className="row">
            <button onClick={onSaveSettings}>Save settings</button>
            {saveMsg && <span className="msg">{saveMsg}</span>}
          </div>
        </section>
      )}

      <section>
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
