import { invoke } from "@tauri-apps/api/core";

export type RuntimeRecord = {
  id: string;
  simcVersion: string;
  build: string;
  gameVersion: string;
  channel: string;
  executableSha256: string;
  installedAtUnixSeconds: number;
};

export type RuntimeView = {
  state: "missing" | "ready" | "damaged";
  active: RuntimeRecord | null;
  installed: RuntimeRecord[];
  availableVersion: string;
  availableBuild: string;
  updateAvailable: boolean;
  diagnostic: string | null;
};

export function runtimeStatus(): Promise<RuntimeView> {
  return invoke<RuntimeView>("runtime_status");
}

export function runtimeInstallLatest(): Promise<RuntimeView> {
  return invoke<RuntimeView>("runtime_install_latest");
}

export function runtimeRollback(): Promise<RuntimeView> {
  return invoke<RuntimeView>("runtime_rollback");
}
