import { invoke } from "@tauri-apps/api/core";
import type { ComposeInfo, OutgoingMode, Settings, SubmitResult } from "./types";

export const hasApiKey = () => invoke<boolean>("has_api_key");
export const setApiKey = (key: string) => invoke<void>("set_api_key", { key });
export const clearApiKey = () => invoke<void>("clear_api_key");
export const getSettings = () => invoke<Settings>("get_settings");
export const saveSettings = (settings: Settings) =>
  invoke<void>("save_settings", { settings });
export const openDataFolder = (sub: "prompts" | "characters" | "logs") =>
  invoke<void>("open_data_folder", { sub });

export const getComposeState = () => invoke<ComposeInfo>("get_compose_state");
export const composeSubmit = (text: string, mode: OutgoingMode) =>
  invoke<SubmitResult>("compose_submit", { text, mode });
export const composeCopyAnyway = () => invoke<void>("compose_copy_anyway");
export const composeCancel = () => invoke<void>("compose_cancel");
