import type { JobView } from "./quick";
import { runKey, type RunReference } from "./runs";
import type { TopGearSessionView } from "./topGear";

export type RunType = "characterAnalysis" | "gearOptimizer";

export type RunCatalogEntry = {
  key: string;
  run: RunReference;
  type: RunType;
  state: string;
  characterName: string;
  specialization: string;
  createdUnixMillis: number;
  updatedUnixMillis: number;
  cpuPreset: JobView["cpuPreset"];
  settings: JobView["settings"];
  job: JobView;
  session: TopGearSessionView | null;
};

const active = (state: string) => state === "queued" || state === "running";
const recoverable = (state: string) => state === "failed" || state === "canceled" || state === "interrupted";
const terminal = (state: string) => !active(state);

export function buildRunCatalog(quickJobs: JobView[], topGearSessions: TopGearSessionView[]) {
  const entries: RunCatalogEntry[] = [
    ...topGearSessions.map((session) => {
      const run = { kind: "topGear", sessionId: session.id } as const;
      return {
        key: runKey(run), run, type: "gearOptimizer" as const,
        state: session.stage === "complete" ? "succeeded" : session.pipelineFailure ? "failed" : session.currentJob.state === "succeeded" ? "running" : session.currentJob.state,
        characterName: session.currentJob.profile.name,
        specialization: session.currentJob.profile.specialization,
        createdUnixMillis: session.createdUnixMillis,
        updatedUnixMillis: session.currentJob.updatedUnixMillis,
        cpuPreset: session.currentJob.cpuPreset,
        settings: session.currentJob.settings,
        job: session.currentJob,
        session,
      };
    }),
    ...quickJobs.map((job) => {
      const run = { kind: "quick", jobId: job.id } as const;
      return {
        key: runKey(run), run, type: "characterAnalysis" as const, state: job.state,
        characterName: job.profile.name, specialization: job.profile.specialization,
        createdUnixMillis: job.createdUnixMillis, updatedUnixMillis: job.updatedUnixMillis,
        cpuPreset: job.cpuPreset, settings: job.settings, job, session: null,
      };
    }),
  ].sort((left, right) => right.createdUnixMillis - left.createdUnixMillis || right.key.localeCompare(left.key));

  return {
    all: entries,
    runs: entries.filter((entry) => active(entry.state) || recoverable(entry.state)),
    history: entries.filter((entry) => terminal(entry.state)),
    results: entries.filter((entry) => entry.state === "succeeded"),
  };
}
