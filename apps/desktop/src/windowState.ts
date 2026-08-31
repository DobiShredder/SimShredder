import { invoke } from "@tauri-apps/api/core";

export function resetWindowState(): Promise<void> {
  return invoke<void>("window_state_reset");
}
