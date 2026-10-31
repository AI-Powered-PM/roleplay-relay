import { useEffect, useState } from "react";
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

function Overlay() {
  const [update, setUpdate] = useState<OverlayUpdate | null>(null);

  useEffect(() => {
    const unlisten = listen<OverlayUpdate>("overlay://update", (e) => setUpdate(e.payload));
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  if (!update) return null;
  const { kind, original, text, back, note, parts, current, style } = update;
  const bottom = style.position.startsWith("bottom");
  return (
    <div className={`stage ${bottom ? "bottom" : "top"}`}>
      <div
        className={`panel ${kind}`}
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
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(<Overlay />);
