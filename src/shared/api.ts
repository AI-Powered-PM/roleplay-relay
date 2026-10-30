import { invoke } from "@tauri-apps/api/core";
import type { Settings } from "./types";

export const hasApiKey = () => invoke<boolean>("has_api_key");
export const setApiKey = (key: string) => invoke<void>("set_api_key", { key });
export const clearApiKey = () => invoke<void>("clear_api_key");
export const getSettings = () => invoke<Settings>("get_settings");
export const saveSettings = (settings: Settings) =>
  invoke<void>("save_settings", { settings });
export const openDataFolder = (sub: "prompts" | "characters" | "logs") =>
  invoke<void>("open_data_folder", { sub });
