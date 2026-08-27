import { invoke } from "@tauri-apps/api/core";

export type SourceFormat = "addonExport" | "simcFile";
export type CpuChoice = "efficient" | "balanced" | "maximum";

export type QuickSimRequest = {
  source: string;
  format: SourceFormat;
  iterations: number;
  fixedTime: boolean;
  maxTimeSeconds: number;
  varyCombatLength: number;
  desiredTargets: number;
  fightStyle: "Patchwerk" | "DungeonSlice" | "HecticAddCleave" | "LightMovement";
  cpuPreset: CpuChoice;
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

export const defaultQuickRequest = (source = "", format: SourceFormat = "addonExport"): QuickSimRequest => ({
  source,
  format,
  iterations: 10_000,
  fixedTime: false,
  maxTimeSeconds: 300,
  varyCombatLength: 0.2,
  desiredTargets: 1,
  fightStyle: "Patchwerk",
  cpuPreset: "balanced",
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
