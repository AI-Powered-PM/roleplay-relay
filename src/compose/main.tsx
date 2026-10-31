import { useCallback, useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import {
  composeCancel,
  composeCopyAnyway,
  composeSubmit,
  getComposeState,
} from "../shared/api";
import type { ComposeFailure, OutgoingMode } from "../shared/types";
import "./styles.css";

const MODES: { id: OutgoingMode; label: string }[] = [
  { id: "direct", label: "Direct" },
  { id: "rp", label: "RP" },
  { id: "grammar", label: "Grammar" },
];

interface Blocked {
  en: string;
  back: string | null;
  reason: string;
}

const asFailure = (e: unknown): ComposeFailure => {
  if (e && typeof e === "object" && "message" in e) {
    return e as ComposeFailure;
  }
  return { message: typeof e === "string" ? e : "Something went wrong", raw: null };
};

function Compose() {
  const [mode, setMode] = useState<OutgoingMode>("direct");
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ComposeFailure | null>(null);
  const [declined, setDeclined] = useState<string | null>(null);
  const [blocked, setBlocked] = useState<Blocked | null>(null);
  const area = useRef<HTMLTextAreaElement>(null);

  const clearFeedback = () => {
    setError(null);
    setDeclined(null);
    setBlocked(null);
  };

  // Rust shows/hides this window; every show arrives as compose://open with the last mode.
  useEffect(() => {
    getComposeState()
      .then((s) => setMode(s.mode))
      .catch(() => {});
    const unlisten = listen<{ mode: OutgoingMode }>("compose://open", (e) => {
      setMode(e.payload.mode);
      setText("");
      setBusy(false);
      clearFeedback();
      setTimeout(() => area.current?.focus(), 0);
    });
    const focusArea = () => area.current?.focus();
    window.addEventListener("focus", focusArea);
    return () => {
      unlisten.then((fn) => fn());
      window.removeEventListener("focus", focusArea);
    };
  }, []);

  const changeMode = useCallback((next: OutgoingMode) => {
    setMode(next);
    setBlocked(null);
    setDeclined(null);
    area.current?.focus();
  }, []);

  const submit = useCallback(async () => {
    if (busy) return;
    if (!text.trim()) {
      setError({ message: "Type a message first", raw: null });
      return;
    }
    setBusy(true);
    clearFeedback();
    try {
      const r = await composeSubmit(text, mode);
      if (r.kind === "declined") setDeclined(r.reason);
      else if (r.kind === "blocked") setBlocked(r);
      // "done": Rust has copied the line and hidden this window. "ignored": nothing to show.
    } catch (e) {
      setError(asFailure(e));
    } finally {
      setBusy(false);
    }
  }, [busy, text, mode]);

  const copyAnyway = async () => {
    try {
      await composeCopyAnyway();
    } catch (e) {
      setError(asFailure(e));
    }
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        composeCancel();
      } else if (e.ctrlKey && ["1", "2", "3"].includes(e.key)) {
        e.preventDefault();
        changeMode(MODES[Number(e.key) - 1].id);
      } else if (e.key === "Tab") {
        e.preventDefault();
        const i = MODES.findIndex((m) => m.id === mode);
        changeMode(MODES[(i + 1) % MODES.length].id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [mode, changeMode]);

  const onAreaKey = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      submit();
    }
  };

  return (
    <div className="card">
      <div className="tabs" role="tablist">
        {MODES.map((m, i) => (
          <button
            key={m.id}
            role="tab"
            aria-selected={mode === m.id}
            className={mode === m.id ? "tab active" : "tab"}
            onClick={() => changeMode(m.id)}
            tabIndex={-1}
          >
            {m.label}
            <span className="key">Ctrl+{i + 1}</span>
          </button>
        ))}
        {busy && <span className="spinner" role="status" aria-label="Working" />}
      </div>

      <textarea
        ref={area}
        className="input"
        value={text}
        autoFocus
        disabled={busy}
        placeholder="Type in Russian or English…"
        spellCheck={false}
        onChange={(e) => {
          setText(e.target.value);
          if (blocked || declined) {
            setBlocked(null);
            setDeclined(null);
          }
        }}
        onKeyDown={onAreaKey}
      />

      {error && (
        <div className="notice error">
          <div>{error.message}</div>
          {error.raw && <div className="raw">{error.raw}</div>}
        </div>
      )}
      {declined && <div className="notice warn">Not written: {declined}</div>}
      {blocked && (
        <div className="notice error">
          <div className="result">{blocked.en}</div>
          <div className="reason">{blocked.reason}</div>
          <button className="copy-anyway" onClick={copyAnyway}>
            Copy anyway
          </button>
        </div>
      )}

      <div className="hint">
        Enter send · Shift+Enter new line · Tab next mode · Esc close
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(<Compose />);
