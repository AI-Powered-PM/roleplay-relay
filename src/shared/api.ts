import { invoke } from "@tauri-apps/api/core";
import type {
  ComposeInfo,
  GameInfo,
  HotkeyStatus,
  LanguageOption,
  OutgoingMode,
  OverlaySettings,
  Provider,
  Settings,
  SubmitResult,
} from "./types";

export const hasApiKey = (provider: Provider) => invoke<boolean>("has_api_key", { provider });
export const setApiKey = (provider: Provider, key: string) =>
  invoke<void>("set_api_key", { provider, key });
export const clearApiKey = (provider: Provider) => invoke<void>("clear_api_key", { provider });
/** Resolves when the saved key works; rejects with the mapped error text. Never sees the key. */
export const testApiKey = (provider: Provider) => invoke<void>("test_api_key", { provider });
export const getSettings = () => invoke<Settings>("get_settings");
export const saveSettings = (settings: Settings) =>
  invoke<HotkeyStatus[]>("save_settings", { settings });
export const getHotkeyStatus = () => invoke<HotkeyStatus[]>("get_hotkey_status");
export const getAutostart = () => invoke<boolean>("get_autostart");
export const setAutostart = (enabled: boolean) => invoke<void>("set_autostart", { enabled });
export const openDataFolder = (sub: "prompts" | "characters" | "logs") =>
  invoke<void>("open_data_folder", { sub });

export const getGameProfile = () => invoke<GameInfo>("get_game_profile");
/** The mode `Convert copied text` uses. */
export const getLastMode = () => invoke<OutgoingMode>("get_last_mode");
export const previewOverlay = (overlay: OverlaySettings, userLanguage: string) =>
  invoke<void>("preview_overlay", { overlay, userLanguage });
export const closeSettings = () => invoke<void>("close_settings");

export const listLanguages = () => invoke<LanguageOption[]>("list_languages");
/** Prompt files that were edited and do not use {{user_language}}. */
export const getPromptLanguageNotes = () => invoke<string[]>("get_prompt_language_notes");

export const getComposeState = () => invoke<ComposeInfo>("get_compose_state");
export const composeSubmit = (text: string, mode: OutgoingMode) =>
  invoke<SubmitResult>("compose_submit", { text, mode });
export const composeCopyAnyway = () => invoke<void>("compose_copy_anyway");
export const composeCancel = () => invoke<void>("compose_cancel");
