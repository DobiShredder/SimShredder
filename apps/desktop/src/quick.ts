import { invoke } from "@tauri-apps/api/core";

export type SourceFormat = "addonExport" | "simcFile";
export type CpuChoice = "efficient" | "balanced" | "maximum";
export type PrecisionChoice = "smart" | "fixed";

export type AnalysisOptions = {
  precision: PrecisionChoice;
  targetError: number;
  targetLevel: number;
  targetRace: "humanoid" | "aberration" | "beast" | "demon" | "dragonkin" | "elemental" | "giant" | "mechanical" | "undead" | "not_specified";
  worldLagMs: number;
  worldLagStddevMs: number;
  playerSkill: number;
  seed: number;
  optimalRaid: boolean;
  bloodlust: boolean;
  bloodlustTime: number;
  bloodlustPercent: number;
  consumables: boolean;
  raidBuffs: {
    arcaneIntellect: boolean;
    battleShout: boolean;
    markOfTheWild: boolean;
    powerWordFortitude: boolean;
    chaosBrand: boolean;
    mysticTouch: boolean;
    windfuryTotem: boolean;
    huntersMark: boolean;
    bleeding: boolean;
  };
  consumableOptions: {
    flask: boolean;
    food: boolean;
    augmentation: boolean;
    potion: boolean;
    temporaryEnchant: boolean;
  };
  reportDetails: boolean;
  reportPetsSeparately: boolean;
  customApl: string;
  customOptions: string;
};

export function detectSourceFormat(source: string): SourceFormat {
  const hasAddonHeader = /^# SimC Addon \S+\s*$/m.test(source);
  const hasRetailHeader = /^# WoW \d+(?:\.\d+){2}\.\d+, TOC \d+\s*$/m.test(source);
  return hasAddonHeader && hasRetailHeader ? "addonExport" : "simcFile";
}

export type QuickSimRequest = {
  source: string;
  format: SourceFormat;
  iterations: number;
  fixedTime: boolean;
  maxTimeSeconds: number;
  varyCombatLength: number;
  desiredTargets: number;
  fightStyle: "Patchwerk" | "CastingPatchwerk" | "DungeonSlice" | "HecticAddCleave" | "LightMovement" | "HeavyMovement" | "HelterSkelter" | "CleaveAdd" | "Beastlord";
  cpuPreset: CpuChoice;
  analysis: AnalysisOptions;
};

export type ProfileSummary = {
  name: string;
  class: string;
  specialization: string;
  race: string;
  role: string;
  level: number;
  equippedItems: number;
  bagItems: number;
  talents: Array<{ name: string; value: string }>;
  warnings: string[];
  inputCompatibility: {
    supportedEditable: number;
    preservedNotEditable: number;
    executionBlocked: number;
    diagnostics: Array<{
      line: number;
      key: string | null;
      category: "supportedEditable" | "preservedNotEditable" | "executionBlocked";
      reason: string;
    }>;
  };
};

export type PreparedQuickSim = {
  profile: ProfileSummary;
  generatedInput: string;
  threads: number;
  profilesetWorkThreads: number;
};

export type AttemptView = {
  id: number;
  sequence: number;
  state: string;
  failure: string | null;
  cacheHit: boolean;
  stdoutLogTruncated: boolean;
  stderrLogTruncated: boolean;
};

export type JobView = {
  id: number;
  state: string;
  cancelRequested: boolean;
  failure: string | null;
  succeededBatches: number;
  pendingBatches: number;
  attempts: AttemptView[];
};

export type StatisticalMetric = {
  name: string;
  mean: number;
  mean_error: number;
  standard_deviation: number;
  minimum: number;
  maximum: number;
  median: number;
};

export type ResultAction = {
  id: number | null;
  name: string;
  internal_name: string;
  school: string;
  executes: number;
  amount_per_fight: number;
  metric_per_second: number;
  share: number;
};

export type ResultBuff = {
  id: number | null;
  name: string;
  internal_name: string;
  uptime_percent: number;
  benefit_percent: number | null;
  starts: number;
};

export type ResultResource = {
  name: string;
  spent_per_fight: number;
  overflow_per_fight: number;
  remaining_per_fight: number;
};

export type ResultAplAction = {
  time_seconds: number;
  id: number | null;
  name: string;
  internal_name: string;
  target: string;
  resources: Record<string, number>;
  resource_max: Record<string, number>;
};

export type NormalizedQuickResult = {
  schema_version: number;
  report_version: string;
  runtime: {
    simc_version: string;
    git_revision: string;
    game_version: string;
    game_build: number;
    channel: string;
  };
  player: {
    name: string;
    race: string;
    role: string;
    specialization: string;
  };
  options: {
    iterations: number;
    threads: number;
    seed: number;
    max_time_seconds: number;
    desired_targets: number;
    fight_style: string;
  };
  primary_metric: StatisticalMetric;
  actions: ResultAction[];
  buffs: ResultBuff[];
  resources: ResultResource[];
  apl_sequence: ResultAplAction[];
};

export type QuickResultView = {
  jobId: number;
  result: NormalizedQuickResult;
  generatedInput: string;
  rawJson: string;
  rawHtml: string;
  stdout: string;
  stderr: string;
  stdoutTruncated: boolean;
  stderrTruncated: boolean;
  artifactDirectory: string;
};

export type ExportView = { directory: string; fileCount: number };

export const defaultQuickRequest = (source = "", format: SourceFormat = detectSourceFormat(source)): QuickSimRequest => ({
  source,
  format,
  iterations: 10_000,
  fixedTime: false,
  maxTimeSeconds: 300,
  varyCombatLength: 0.2,
  desiredTargets: 1,
  fightStyle: "Patchwerk",
  cpuPreset: "balanced",
  analysis: {
    precision: "fixed",
    targetError: 0.2,
    targetLevel: 93,
    targetRace: "humanoid",
    worldLagMs: 50,
    worldLagStddevMs: 5,
    playerSkill: 1,
    seed: 1,
    optimalRaid: true,
    bloodlust: true,
    bloodlustTime: 0,
    bloodlustPercent: 0,
    consumables: true,
    raidBuffs: {
      arcaneIntellect: true,
      battleShout: true,
      markOfTheWild: true,
      powerWordFortitude: true,
      chaosBrand: true,
      mysticTouch: true,
      windfuryTotem: true,
      huntersMark: true,
      bleeding: true,
    },
    consumableOptions: {
      flask: true,
      food: true,
      augmentation: true,
      potion: true,
      temporaryEnchant: true,
    },
    reportDetails: true,
    reportPetsSeparately: false,
    customApl: "",
    customOptions: "",
  },
});

export function quickPrepare(request: QuickSimRequest): Promise<PreparedQuickSim> {
  return invoke("quick_prepare", { request });
}

export function quickStart(request: QuickSimRequest): Promise<JobView> {
  return invoke("quick_start", { request });
}

export function quickJobStatus(jobId: number): Promise<JobView> {
  return invoke("quick_job_status", { jobId });
}

export function quickCancel(jobId: number): Promise<JobView> {
  return invoke("quick_cancel", { jobId });
}

export function quickRetry(jobId: number): Promise<JobView> {
  return invoke("quick_retry", { jobId });
}

export function quickResult(jobId: number): Promise<QuickResultView> {
  return invoke("quick_result", { jobId });
}

export function quickExport(jobId: number): Promise<ExportView> {
  return invoke("quick_export", { jobId });
}

export function quickRecover(): Promise<number[]> {
  return invoke("quick_recover");
}

export function quickJobs(): Promise<JobView[]> {
  return invoke("quick_jobs");
}

export function quickDelete(jobId: number): Promise<void> {
  return invoke("quick_delete", { jobId });
}
