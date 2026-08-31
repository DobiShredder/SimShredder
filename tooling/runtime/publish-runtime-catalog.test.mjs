import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { promoteStagedAsset, promotionNames } from "./publish-runtime-catalog.mjs";

test("uses content-addressed staging and backup names without exposing secrets", () => {
  const bytes = Buffer.from("signed catalog fixture");
  const digest = createHash("sha256").update(bytes).digest("hex");
  assert.deepEqual(promotionNames(bytes), {
    canonical: "runtime-catalog.json",
    staged: `runtime-catalog.${digest}.staged.json`,
    backup: `runtime-catalog.${digest}.backup.json`,
    digest,
  });
  assert.equal(JSON.stringify(promotionNames(bytes)).includes(process.env.GH_TOKEN ?? "not-present"), false);
});

test("restores the previous canonical asset when staged promotion fails", async () => {
  const calls = [];
  const failure = new Error("injected promotion failure");
  await assert.rejects(promoteStagedAsset({
    canonical: { id: 1 },
    staged: { id: 2 },
    names: { canonical: "runtime-catalog.json", backup: "backup.json" },
    rename: async (id, name) => {
      calls.push(["rename", id, name]);
      if (id === 2) throw failure;
    },
    remove: async (id) => calls.push(["remove", id]),
  }), failure);
  assert.deepEqual(calls, [
    ["rename", 1, "backup.json"],
    ["rename", 2, "runtime-catalog.json"],
    ["rename", 1, "runtime-catalog.json"],
  ]);
});

test("workflow keeps signing secrets out of commands, artifacts and unchanged jobs", async () => {
  const workflow = await readFile(".github/workflows/publish-runtime-catalog.yml", "utf8");
  assert.match(workflow, /needs\.discovery\.outputs\.changed == 'true'/);
  assert.match(workflow, /vars\.RUNTIME_CATALOG_PUBLISH_ENABLED == 'true'/);
  assert.match(workflow, /RUNTIME_CATALOG_SIGNING_KEY_PEM: \$\{\{ secrets\.RUNTIME_CATALOG_SIGNING_KEY_PEM \}\}/);
  assert.match(workflow, /umask 077/);
  assert.match(workflow, /trap cleanup EXIT/);
  assert.match(workflow, /node tooling\/runtime\/sign-runtime-catalog\.mjs/);
  assert.doesNotMatch(workflow, /cargo run[\s\S]{0,200}sign-runtime-catalog/);
  assert.doesNotMatch(workflow, /set -x|echo ["']?\$RUNTIME_CATALOG_SIGNING_KEY_PEM/);
  assert.doesNotMatch(workflow, /upload-artifact[\s\S]{0,500}runtime-catalog-signing-key/);
});

test("desktop never treats unsigned nightly discovery as an install source", async () => {
  const desktop = await readFile("apps/desktop/src-tauri/src/lib.rs", "utf8");
  assert.doesNotMatch(desktop, /discover_latest_macos|nightly-listing|parseNightlyListing/);
  assert.match(desktop, /verify_and_accept_catalog_for_target/);
  assert.match(desktop, /replacement_for_removed_manifest/);
});
