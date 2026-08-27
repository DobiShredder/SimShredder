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
  activeDataDate: string | null;
  installed: RuntimeRecord[];
  availableVersion: string;
  availableBuild: string;
  updateAvailable: boolean;
  diagnostic: string | null;
};

export function runtimeStatus(): Promise<RuntimeView> {
  return invoke<RuntimeView>("runtime_status");
}

export function runtimeCheckUpdates(): Promise<RuntimeView> {
  return invoke<RuntimeView>("runtime_check_updates");
}

export function runtimeInstallLatest(): Promise<RuntimeView> {
  return invoke<RuntimeView>("runtime_install_latest");
}

export function runtimeRollback(): Promise<RuntimeView> {
  return invoke<RuntimeView>("runtime_rollback");
}

export function formatRuntimeDataDate(value: string | null, locale: string): string | null {
  if (!value || !/^\d{4}-\d{2}-\d{2}$/.test(value)) return null;
  const [year, month, day] = value.split("-").map(Number);
  return new Intl.DateTimeFormat(locale, {
    year: "numeric",
    month: "short",
    day: "numeric",
    timeZone: "UTC",
  }).format(new Date(Date.UTC(year, month - 1, day)));
}
