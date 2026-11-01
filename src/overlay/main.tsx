import { useLayoutEffect, useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import "./styles.css";

type Kind = "busy" | "result" | "error" | "info";

interface OverlayUpdate {
  kind: Kind;
  original: string | null;
  text: string;
  back: string | null;
  note: string | null;
  parts: string[] | null;
  current: number | null;
  style: {
    fontSize: number;
    opacity: number;
    position: "top-right" | "top-left" | "bottom-right" | "bottom-left";
  };
}

interface HistoryPayload {
  entries: OverlayUpdate[];
}

function Entry({ update, newest }: { update: OverlayUpdate; newest: boolean }) {
  const { kind, original, text, back, note, parts, current, style } = update;
  const ref = useRef<HTMLDivElement>(null);
  const [clipped, setClipped] = useState(false);
  // The newest entry is clipped at the bottom when it is taller than the window; fade that edge.
  useLayoutEffect(() => {
    const check = () => {
      const el = ref.current;
      setClipped(newest && !!el && el.scrollHeight > el.clientHeight + 1);
    };
    check();
    // place() resizes the window after the update arrives, so measure again at the final size.
    window.addEventListener("resize", check);
    return () => window.removeEventListener("resize", check);
  }, [update, newest]);
  return (
    <div
      ref={ref}
      className={`panel ${kind}${newest ? " newest" : ""}${clipped ? " clipped" : ""}`}
      style={{
        fontSize: style.fontSize,
        background: `rgba(15, 15, 20, ${style.opacity})`,
      }}
    >
      {note && <div className="note">{note}</div>}
      {original && <div className="original">{original}</div>}
      {parts && parts.length > 1 ? (
        <ol className="parts">
          {parts.map((part, i) => (
            <li
              key={i}
              className={i === current ? "part current" : i < (current ?? 0) ? "part done" : "part"}
            >
              {part}
            </li>
          ))}
        </ol>
      ) : (
        <div className="text">{text}</div>
      )}
      {back && <div className="back">{back}</div>}
    </div>
  );
}

function Overlay() {
  // Oldest first, newest last. A normal update is a list of one; the toggle sends the history.
  const [entries, setEntries] = useState<OverlayUpdate[]>([]);

  useEffect(() => {
    const subs = [
      listen<OverlayUpdate>("overlay://update", (e) => setEntries([e.payload])),
      listen<HistoryPayload>("overlay://history", (e) => setEntries(e.payload.entries)),
    ];
    return () => {
      subs.forEach((p) => p.then((fn) => fn()));
    };
  }, []);

  if (entries.length === 0) return null;
  const bottom = entries[entries.length - 1].style.position.startsWith("bottom");
  return (
    <div className={`stage ${bottom ? "bottom" : "top"}`}>
      <div className="stack">
        {entries.map((u, i) => (
          <Entry key={i} update={u} newest={i === entries.length - 1} />
        ))}
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(<Overlay />);
