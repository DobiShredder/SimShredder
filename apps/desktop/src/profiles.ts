import { invoke } from "@tauri-apps/api/core";
import type { QuickSimRequest } from "./quick";

export type ProfileInputSource = "addonExport" | "simcFile" | "armory";

export type CharacterProfile = {
  id: string;
  identity: {
    region: string | null;
    realm: string | null;
    characterName: string;
  };
  displayName: string;
  class: string;
  specialization: string;
  favorite: boolean;
  inputSource: ProfileInputSource;
  request: QuickSimRequest;
  capturedAtUnixSeconds: number;
  previousInputAvailable: boolean;
  armoryRefresh: {
    available: boolean;
    reason: string;
  };
};

export function characterProfiles(): Promise<CharacterProfile[]> {
  return invoke("character_profiles");
}

export function saveCharacterProfileImport(request: QuickSimRequest): Promise<CharacterProfile> {
  return invoke("character_profile_save_import", { request });
}

export function setCharacterProfileFavorite(profileId: string, favorite: boolean): Promise<CharacterProfile> {
  return invoke("character_profile_set_favorite", { profileId, favorite });
}

export function deleteCharacterProfile(profileId: string): Promise<void> {
  return invoke("character_profile_delete", { profileId });
}

export function reloadCharacterProfileFromArmory(profileId: string): Promise<CharacterProfile> {
  return invoke("character_profile_reload_armory", { profileId });
}

export function restorePreviousCharacterProfileInput(profileId: string): Promise<CharacterProfile> {
  return invoke("character_profile_restore_previous", { profileId });
}
