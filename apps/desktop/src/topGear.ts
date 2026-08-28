import { invoke } from "@tauri-apps/api/core";
import type { JobView, QuickSimRequest } from "./quick";

export type GearSlot =
  | "head" | "neck" | "shoulders" | "back" | "chest" | "shirt" | "tabard"
  | "wrists" | "hands" | "waist" | "legs" | "feet" | "finger1" | "finger2"
  | "trinket1" | "trinket2" | "main_hand" | "off_hand";

export type CostVector = Record<string, number>;
export type ChangeKind = "equip" | "gem" | "enchant" | "upgrade" | "catalyst";
export type WeaponKind = "none" | "one_hand" | "two_hand" | "off_hand";

export type UpgradeAction = {
  id: string;
  label: string;
  kind: ChangeKind;
  cost: CostVector;
  dependsOn: string[];
  fromRank: number | null;
  toRank: number | null;
  slot: GearSlot;
  sourceItemId: number;
  simcOptionsPatch: Record<string, string>;
};

export type ItemVariant = {
  key: string;
  sourceItemId: number;
  slot: GearSlot;
  displayName: string | null;
  rank: number;
  gemIds: number[];
  enchantId: number | null;
  simcOptions: Record<string, string>;
  cost: CostVector;
  actions: UpgradeAction[];
  uniqueGroups: string[];
  setGroups: string[];
  weaponKind: WeaponKind;
  embellishment: boolean;
  catalyst: boolean;
  enabled: boolean;
  changed: boolean;
};

export type TalentVariant = {
  key: string;
  label: string;
  option: string;
  value: string;
  changed: boolean;
  enabled: boolean;
};

export type ProfileOptionVariant = TalentVariant;

export type Loadout = {
  key: string;
  items: Partial<Record<GearSlot, ItemVariant>>;
  cost: CostVector;
  changedSlots: number;
  changedOptions: number;
  talent: TalentVariant;
  profileOptions: Record<string, ProfileOptionVariant>;
};

export type TopGearRequest = {
  quick: QuickSimRequest;
  variants: ItemVariant[];
  talentLoadouts: TalentVariant[];
  profileOptions: Record<string, ProfileOptionVariant[]>;
  lockedSlots: GearSlot[];
  minimumSetPieces: Record<string, number>;
  catalystCharges: number;
  balances: CostVector;
  reserves: CostVector;
  currencyConfirmedAtUnixSeconds: number;
  ruleRevision: string;
  gameBuild: number;
  combinationLimit: number;
  lowIterations: number;
  highIterations: number;
  finalistCount: number;
};

export type PreparedTopGear = {
  profileName: string;
  ruleRevision: string;
  ruleSource: string;
  rawCombinations: number;
  validCombinations: number;
  executionCount: number;
  finalistCount: number;
  estimated: boolean;
  rejections: {
    dominatedVariants: number;
    socketLimit: number;
    enchantSlot: number;
    uniqueEquipped: number;
    embellishmentLimit: number;
    weaponConstraint: number;
    budget: number;
    symmetricDuplicate: number;
    minimumSetBonus: number;
    catalystLimit: number;
  };
  generatedInput: string;
  variants: ItemVariant[];
  talentLoadouts: TalentVariant[];
  profileOptions: Record<string, ProfileOptionVariant[]>;
  loadouts: Loadout[];
};

export type TopGearSessionView = {
  id: string;
  stage: "low_precision" | "high_precision" | "action_plan" | "complete";
  currentJob: JobView;
  lowJobId: number;
  highJobId: number | null;
  actionJobId: number | null;
  completedExecutions: number;
  totalExecutions: number;
  canAdvance: boolean;
};

export type PlannedAction = {
  id: string;
  label: string;
  kind: ChangeKind;
  cost: CostVector;
  remaining: CostVector;
  marginalGain: number;
  cumulativeGain: number;
};

export type RankedLoadout = {
  loadout: Loadout;
  mean: number;
  meanError: number;
  delta: number;
  combinedError: number;
  equivalentToBaseline: boolean;
  paretoOptimal: boolean;
  rank: number;
};

export type TopGearResultView = {
  sessionId: string;
  baselineKey: string;
  ruleRevision: string;
  ranked: RankedLoadout[];
  lowJobId: number;
  highJobId: number;
  actionJobId: number | null;
  actionPlan: PlannedAction[];
  estimated: boolean;
  finalGeneratedInput: string;
  runtime: {
    simc_version: string;
    git_revision: string;
    game_version: string;
    game_build: number;
    channel: string;
  };
  budget: { balances: CostVector; reserves: CostVector; confirmedAtUnixSeconds: number };
};

export function defaultTopGearRequest(quick: QuickSimRequest): TopGearRequest {
  return {
    quick,
    variants: [],
    talentLoadouts: [],
    profileOptions: {},
    lockedSlots: [],
    minimumSetPieces: {},
    catalystCharges: 0,
    balances: { champion_mistcrest: 0, hero_mistcrest: 0, myth_mistcrest: 0, spark_of_tides: 0 },
    reserves: { champion_mistcrest: 0, hero_mistcrest: 0, myth_mistcrest: 0, spark_of_tides: 0 },
    currencyConfirmedAtUnixSeconds: Math.floor(Date.now() / 1000),
    ruleRevision: "12.1.0-69465-v1",
    gameBuild: 69465,
    combinationLimit: 1_024,
    lowIterations: 1_000,
    highIterations: 10_000,
    finalistCount: 8,
  };
}

export const topGearPrepare = (request: TopGearRequest): Promise<PreparedTopGear> =>
  invoke("top_gear_prepare", { request });
export const topGearStart = (request: TopGearRequest): Promise<TopGearSessionView> =>
  invoke("top_gear_start", { request });
export const topGearStatus = (sessionId: string): Promise<TopGearSessionView> =>
  invoke("top_gear_status", { sessionId });
export const topGearAdvance = (sessionId: string): Promise<TopGearSessionView> =>
  invoke("top_gear_advance", { sessionId });
export const topGearCancel = (sessionId: string): Promise<TopGearSessionView> =>
  invoke("top_gear_cancel", { sessionId });
export const topGearRetry = (sessionId: string): Promise<TopGearSessionView> =>
  invoke("top_gear_retry", { sessionId });
export const topGearResult = (sessionId: string): Promise<TopGearResultView> =>
  invoke("top_gear_result", { sessionId });
export const topGearExport = (sessionId: string): Promise<{ directory: string; fileCount: number }> =>
  invoke("top_gear_export", { sessionId });
export const topGearSessions = (): Promise<TopGearSessionView[]> => invoke("top_gear_sessions");
export const topGearDelete = (sessionId: string): Promise<void> => invoke("top_gear_delete", { sessionId });
