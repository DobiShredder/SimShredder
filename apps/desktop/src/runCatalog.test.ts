import { describe, expect, it } from "vitest";
import type { JobView } from "./quick";
import { buildRunCatalog } from "./runCatalog";
import type { TopGearSessionView } from "./topGear";

const job = (id: number, state: string, createdUnixMillis: number): JobView => ({
  id, state, cancelRequested: false, failure: null, succeededBatches: state === "succeeded" ? 1 : 0,
  pendingBatches: state === "succeeded" ? 0 : 1, attempts: [], createdUnixMillis,
  updatedUnixMillis: createdUnixMillis + 1_000, cpuPreset: "balanced",
  profile: { name: "Core", class: "warrior", specialization: "fury" },
  settings: { iterations: 10_000, maxTimeSeconds: 300, desiredTargets: 1, fightStyle: "Patchwerk", threads: 4 },
  recentDiagnostics: [],
});

describe("shared run catalog", () => {
  it("separates active/recoverable runs, terminal history, and successful results", () => {
    const running = job(1, "running", 100);
    const failed = job(2, "failed", 200);
    const succeeded = job(3, "succeeded", 300);
    const catalog = buildRunCatalog([succeeded, failed, running], []);

    expect(catalog.runs.map(({ key }) => key)).toEqual(["quick-2", "quick-1"]);
    expect(catalog.history.map(({ key }) => key)).toEqual(["quick-3", "quick-2"]);
    expect(catalog.results.map(({ key }) => key)).toEqual(["quick-3"]);
  });

  it("uses the session identity and low-stage creation time for Gear Optimizer", () => {
    const currentJob = job(12, "succeeded", 900);
    const session: TopGearSessionView = {
      id: "tg-1", stage: "complete", currentJob, lowJobId: 10, mediumJobId: 11,
      highJobId: 12, actionJobId: null, completedExecutions: 9, totalExecutions: 9,
      canAdvance: false, pipelineFailure: null, createdUnixMillis: 400,
    };

    const catalog = buildRunCatalog([], [session]);
    expect(catalog.results[0]).toMatchObject({ key: "top-gear-tg-1", createdUnixMillis: 400, characterName: "Core", specialization: "fury" });
  });
});
