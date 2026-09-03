import type { QuickSimRequest } from "./quick";
import type { RunReference } from "./runs";
import { runKey } from "./runs";
import type { TopGearRequest } from "./topGear";

const STORAGE_KEY = "simshredder.workflowPreferences";
const SCHEMA_VERSION = 1;
const MAX_BYTES = 2 * 1024 * 1024;
const MAX_PRESETS_PER_PROFILE = 32;
const MAX_RUNS = 200;

export type WorkflowKind = "quick" | "topGear";
export type StoredRunDraft =
  | { kind: "quick"; profileId: string | null; request: QuickSimRequest }
  | { kind: "topGear"; profileId: string | null; request: TopGearRequest };

export type WorkflowPreset = {
  id: string;
  kind: WorkflowKind;
  name: string;
  request: QuickSimRequest | TopGearRequest;
  updatedAtUnixMillis: number;
};

type ProfilePreferences = {
  lastQuick: QuickSimRequest | null;
  lastTopGear: TopGearRequest | null;
  presets: WorkflowPreset[];
};

type RunPreferences = {
  name: string | null;
  draft: StoredRunDraft | null;
  updatedAtUnixMillis: number;
};

type PreferencesDocument = {
  schemaVersion: 1;
  profiles: Record<string, ProfilePreferences>;
  runs: Record<string, RunPreferences>;
};

const emptyDocument = (): PreferencesDocument => ({ schemaVersion: SCHEMA_VERSION, profiles: {}, runs: {} });

function read(): PreferencesDocument {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw || raw.length > MAX_BYTES) return emptyDocument();
    const value = JSON.parse(raw) as { schemaVersion?: number; profiles?: PreferencesDocument["profiles"]; runs?: PreferencesDocument["runs"] };
    if (value.schemaVersion === 0 && value.profiles) {
      const migrated: PreferencesDocument = { schemaVersion: SCHEMA_VERSION, profiles: value.profiles, runs: {} };
      write(migrated);
      return migrated;
    }
    if (value.schemaVersion !== SCHEMA_VERSION || !value.profiles || !value.runs) return emptyDocument();
    return value as PreferencesDocument;
  } catch {
    return emptyDocument();
  }
}

function write(document: PreferencesDocument) {
  const orderedRuns = Object.entries(document.runs)
    .sort(([, left], [, right]) => right.updatedAtUnixMillis - left.updatedAtUnixMillis)
    .slice(0, MAX_RUNS);
  document.runs = Object.fromEntries(orderedRuns);
  const raw = JSON.stringify(document);
  if (raw.length > MAX_BYTES) throw new Error("workflow preferences exceed the local storage limit");
  // One versioned value gives setItem an atomic old-or-new document boundary.
  window.localStorage.setItem(STORAGE_KEY, raw);
}

function profile(document: PreferencesDocument, profileId: string): ProfilePreferences {
  return document.profiles[profileId] ?? { lastQuick: null, lastTopGear: null, presets: [] };
}

export function loadLastRequest(profileId: string, kind: "quick"): QuickSimRequest | null;
export function loadLastRequest(profileId: string, kind: "topGear"): TopGearRequest | null;
export function loadLastRequest(profileId: string, kind: WorkflowKind) {
  const stored = profile(read(), profileId);
  return kind === "quick" ? stored.lastQuick : stored.lastTopGear;
}

export function saveLastRequest(profileId: string, draft: StoredRunDraft) {
  const document = read();
  const stored = profile(document, profileId);
  document.profiles[profileId] = draft.kind === "quick"
    ? { ...stored, lastQuick: draft.request }
    : { ...stored, lastTopGear: draft.request };
  write(document);
}

export function listPresets(profileId: string, kind: WorkflowKind) {
  return profile(read(), profileId).presets
    .filter((preset) => preset.kind === kind)
    .sort((left, right) => right.updatedAtUnixMillis - left.updatedAtUnixMillis);
}

export function savePreset(profileId: string, preset: Omit<WorkflowPreset, "id" | "updatedAtUnixMillis"> & { id?: string }) {
  const document = read();
  const stored = profile(document, profileId);
  const name = preset.name.trim().slice(0, 80);
  if (!name) throw new Error("preset name is required");
  const id = preset.id ?? `${preset.kind}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
  const next: WorkflowPreset = { ...preset, id, name, updatedAtUnixMillis: Date.now() };
  const presets = [next, ...stored.presets.filter((candidate) => candidate.id !== id)].slice(0, MAX_PRESETS_PER_PROFILE);
  document.profiles[profileId] = { ...stored, presets };
  write(document);
  return next;
}

export function deletePreset(profileId: string, presetId: string) {
  const document = read();
  const stored = profile(document, profileId);
  document.profiles[profileId] = { ...stored, presets: stored.presets.filter((preset) => preset.id !== presetId) };
  write(document);
}

export function saveRunDraft(run: RunReference, draft: StoredRunDraft) {
  const document = read();
  const key = runKey(run);
  document.runs[key] = { name: document.runs[key]?.name ?? null, draft, updatedAtUnixMillis: Date.now() };
  write(document);
}

export function loadRunDraft(run: RunReference) {
  return read().runs[runKey(run)]?.draft ?? null;
}

export function hasRunDraft(run: RunReference) {
  return read().runs[runKey(run)]?.draft != null;
}

export function renameRun(run: RunReference, name: string | null) {
  const document = read();
  const key = runKey(run);
  const current = document.runs[key] ?? { name: null, draft: null, updatedAtUnixMillis: 0 };
  const normalized = name?.trim().slice(0, 80) || null;
  document.runs[key] = { ...current, name: normalized, updatedAtUnixMillis: Date.now() };
  write(document);
}

export function runNames() {
  return Object.fromEntries(Object.entries(read().runs).flatMap(([key, value]) => value.name ? [[key, value.name]] : []));
}

export function removeRunPreferences(run: RunReference) {
  const document = read();
  delete document.runs[runKey(run)];
  write(document);
}

export const workflowPreferencesTest = { read, emptyDocument, storageKey: STORAGE_KEY };
