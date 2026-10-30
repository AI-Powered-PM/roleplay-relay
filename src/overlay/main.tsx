import { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import "./styles.css";

type Kind = "busy" | "result" | "error" | "info";

interface OverlayUpdate {
  kind: Kind;
  original: string | null;
  text: string;
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
  const { kind, original, text, style } = update;
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
        {original && <div className="original">{original}</div>}
        <div className="text">{text}</div>
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(<Overlay />);
