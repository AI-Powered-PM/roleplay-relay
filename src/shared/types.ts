export type Mode = "incoming" | "direct" | "rp" | "grammar";

export type Provider = "anthropic" | "openai";

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
  provider: Provider;
  /** One model table per provider; switching provider keeps both. */
  models: Record<Provider, Record<Mode, ModeConfig>>;
  overlay: OverlaySettings;
  splitMarker: "(n/m)" | "…" | "none";
  debugLogChat: boolean;
  activeCharacter: string;
  /** Language code from list_languages(), e.g. "ru". */
  userLanguage: string;
}

export interface LanguageOption {
  code: string;
  label: string;
}

export type OutgoingMode = "direct" | "rp" | "grammar";

export interface ComposeInfo {
  mode: OutgoingMode;
}

export interface ComposeFailure {
  message: string;
  /** The model's raw reply when it was not in the expected shape (shown dimmed). */
  raw: string | null;
}

export type SubmitResult =
  | { kind: "done" }
  | { kind: "ignored" }
  | { kind: "declined"; reason: string }
  | { kind: "blocked"; en: string; back: string | null; reason: string };

export interface HotkeyStatus {
  action: string;
  ok: boolean;
  error: string | null;
}
