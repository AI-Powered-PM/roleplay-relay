export type Mode = "incoming" | "direct" | "rp" | "grammar";

export interface ModeConfig {
  model: string;
  maxTokens: number;
  temperature: number | null;
}

export interface OverlaySettings {
  position: "top-right" | "top-left" | "bottom-right" | "bottom-left";
  width: number;
  fontSize: number;
  opacity: number;
  autoHideSeconds: number;
}

export interface Settings {
  hotkeys: Record<string, string>;
  modes: Record<Mode, ModeConfig>;
  overlay: OverlaySettings;
  splitMarker: "(n/m)" | "…" | "none";
  debugLogChat: boolean;
  activeCharacter: string;
}
