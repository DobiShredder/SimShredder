import { invoke } from "@tauri-apps/api/core";

export type StoragePaths = {
  configRoot: string;
  workspace: string;
  simulationcraft: string;
  icons: string;
  exports: string;
  defaultWorkspace: string;
  defaultSimulationcraft: string;
  defaultIcons: string;
  defaultExports: string;
};

export type StoragePathsRequest = Pick<StoragePaths, "workspace" | "simulationcraft" | "icons" | "exports">;

export const storagePathsGet = (): Promise<StoragePaths> => invoke("storage_paths_get");

export const storagePathsSave = (request: StoragePathsRequest): Promise<StoragePaths> =>
  invoke("storage_paths_save", { request });

export const storagePathsReset = (): Promise<StoragePaths> => invoke("storage_paths_reset");
