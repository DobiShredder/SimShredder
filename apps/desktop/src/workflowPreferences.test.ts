import { beforeEach, describe, expect, it } from "vitest";
import { defaultQuickRequest } from "./quick";
import {
  deletePreset,
  listPresets,
  loadLastRequest,
  loadRunDraft,
  renameRun,
  runNames,
  saveLastRequest,
  savePreset,
  saveRunDraft,
  workflowPreferencesTest,
} from "./workflowPreferences";

describe("workflow preferences", () => {
  beforeEach(() => window.localStorage.clear());

  it("atomically restores profile settings and bounded user presets", () => {
    const request = { ...defaultQuickRequest("warrior=Test"), desiredTargets: 5 };
    saveLastRequest("profile-1", { kind: "quick", profileId: "profile-1", request });
    expect(loadLastRequest("profile-1", "quick")?.desiredTargets).toBe(5);
    const preset = savePreset("profile-1", { kind: "quick", name: "  My AoE  ", request });
    expect(listPresets("profile-1", "quick")[0]).toMatchObject({ id: preset.id, name: "My AoE" });
    deletePreset("profile-1", preset.id);
    expect(listPresets("profile-1", "quick")).toEqual([]);
  });

  it("keeps an immutable run draft separate from its editable display name", () => {
    const run = { kind: "quick" as const, jobId: 42 };
    const request = defaultQuickRequest("warrior=Original");
    saveRunDraft(run, { kind: "quick", profileId: "profile-1", request });
    renameRun(run, "  Progression ST  ");
    expect(loadRunDraft(run)).toEqual({ kind: "quick", profileId: "profile-1", request });
    expect(runNames()).toEqual({ "quick-42": "Progression ST" });
  });

  it("fails closed on an unsupported schema", () => {
    window.localStorage.setItem(workflowPreferencesTest.storageKey, JSON.stringify({ schemaVersion: 99, profiles: {}, runs: {} }));
    expect(workflowPreferencesTest.read()).toEqual(workflowPreferencesTest.emptyDocument());
  });

  it("migrates the initial profile-only document atomically", () => {
    window.localStorage.setItem(workflowPreferencesTest.storageKey, JSON.stringify({ schemaVersion: 0, profiles: {} }));
    expect(workflowPreferencesTest.read()).toEqual({ schemaVersion: 1, profiles: {}, runs: {} });
    expect(JSON.parse(window.localStorage.getItem(workflowPreferencesTest.storageKey)!)).toEqual({ schemaVersion: 1, profiles: {}, runs: {} });
  });
});
