import { invoke } from "@tauri-apps/api/core";

export interface IconCacheStatus {
  budgetBytes: number;
  usedBytes: number;
  iconCount: number;
  mappingCount: number;
  remoteProviderEnabled: boolean;
}

export const iconCacheStatus = () => invoke<IconCacheStatus>("icon_cache_status");
export const iconCacheClear = () => invoke<IconCacheStatus>("icon_cache_clear");
