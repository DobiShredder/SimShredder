import { readdir, readFile } from "node:fs/promises";
import path from "node:path";

const workflowDirectory = ".github/workflows";
const files = (await readdir(workflowDirectory))
  .filter((name) => name.endsWith(".yml") || name.endsWith(".yaml"))
  .sort();

if (files.length === 0) throw new Error("no GitHub Actions workflows found");

let actionCount = 0;
let checkoutCount = 0;
for (const name of files) {
  const file = path.join(workflowDirectory, name);
  const source = await readFile(file, "utf8");
  if (/^\s*(push|pull_request)\s*:/m.test(source)) {
    throw new Error(`${file} must not run automatically for commits, pushes, or pull requests`);
  }
  if (/^\s*pull_request_target\s*:/m.test(source)) {
    throw new Error(`${file} must not execute repository code through pull_request_target`);
  }

  for (const match of source.matchAll(/^\s*-\s+uses:\s+([^@\s]+)@([^\s#]+)/gm)) {
    actionCount += 1;
    const [, action, revision] = match;
    if (!/^[0-9a-f]{40}$/.test(revision)) {
      throw new Error(`${file} uses mutable or non-commit action reference ${action}@${revision}`);
    }
    if (action === "actions/checkout") {
      checkoutCount += 1;
      const following = source.slice(match.index, match.index + 320);
      if (!/persist-credentials:\s*false/.test(following)) {
        throw new Error(`${file} checkout must disable credential persistence`);
      }
    }
  }

  if (source.includes("contents: write")) {
    if (!source.includes("environment: release") || !/^\s*workflow_dispatch\s*:/m.test(source)) {
      throw new Error(`${file} write permission must be behind workflow_dispatch and the release environment`);
    }
    if (/^\s*(push|pull_request)\s*:/m.test(source)
      || /^\s*schedule\s*:/m.test(source)) {
      throw new Error(`${file} write permission must not be reachable from an unauthorized automatic event`);
    }
    if (!source.includes("github.ref == 'refs/heads/master'")) {
      throw new Error(`${file} write permission must be restricted to the master ref`);
    }
    if (!source.includes("audit-public-history.mjs")) {
      throw new Error(`${file} write permission must be preceded by the public-history audit`);
    }
  }
  if (source.includes("${{ secrets.") && !source.includes("environment: release")) {
    throw new Error(`${file} references secrets outside the release environment`);
  }
}

const prepareRelease = await readFile(path.join(workflowDirectory, "unsigned-release.yml"), "utf8");
if (prepareRelease.includes("contents: write") || prepareRelease.includes("gh release create")) {
  throw new Error("unsigned release preparation must not publish or receive contents:write");
}
if (!prepareRelease.includes("audit-public-history.mjs")) {
  throw new Error("unsigned release preparation must audit public history before native builds");
}
const publishRelease = await readFile(path.join(workflowDirectory, "publish-unsigned-release.yml"), "utf8");
for (const requiredBoundary of [
  "verify-unsigned-candidate.mjs",
  "verify-manual-release-evidence.mjs",
  "candidate_run_id",
  "manual_evidence_base64",
  "run-id: ${{ inputs.candidate_run_id }}",
]) {
  if (!publishRelease.includes(requiredBoundary)) {
    throw new Error(`unsigned release publisher is missing required boundary: ${requiredBoundary}`);
  }
}
const continuousIntegration = await readFile(path.join(workflowDirectory, "ci.yml"), "utf8");
for (const requiredCostBoundary of [
  "run_native:",
  "if: inputs.run_native",
  "if: inputs.run_native && inputs.capture_gui_baselines != true",
  "capture_gui_baselines requires run_native",
  "SIMSHREDDER_E2E_CI_BASELINE_CAPTURE: ${{ inputs.run_native && inputs.capture_gui_baselines && '1' || '0' }}",
]) {
  if (!continuousIntegration.includes(requiredCostBoundary)) {
    throw new Error(`CI is missing the hosted-runner cost boundary: ${requiredCostBoundary}`);
  }
}
const desktopE2e = await readFile("apps/desktop/config/wdio.conf.ts", "utf8");
for (const requiredCaptureBoundary of [
  'process.env.SIMSHREDDER_E2E_CI_BASELINE_CAPTURE === "1"',
  "acceptBaseline && process.env.CI && !authorizedCiCapture",
  "authorizedCiCapture && (!process.env.CI || !acceptBaseline)",
]) {
  if (!desktopE2e.includes(requiredCaptureBoundary)) {
    throw new Error(`desktop E2E is missing the CI baseline capture boundary: ${requiredCaptureBoundary}`);
  }
}
if (actionCount === 0 || checkoutCount === 0) throw new Error("workflow action audit inspected no actions");
process.stdout.write(`verified ${actionCount} pinned action steps and ${checkoutCount} non-persistent checkouts across ${files.length} workflows\n`);
