import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const audit = path.join(path.dirname(fileURLToPath(import.meta.url)), "audit-public-history.mjs");

function git(root, ...args) {
  return execFileSync("git", args, { cwd: root, encoding: "utf8" });
}

async function repository(context) {
  const root = await mkdtemp(path.join(os.tmpdir(), "simshredder-history-audit-"));
  context.after(() => rm(root, { recursive: true }));
  git(root, "init", "-b", "master");
  git(root, "config", "user.name", "SimShredder Test");
  git(root, "config", "user.email", "test@users.noreply.github.com");
  return root;
}

function commit(root, message) {
  git(root, "add", "--all");
  git(root, "commit", "-m", message);
}

function runAudit(root) {
  return spawnSync(process.execPath, [audit], { cwd: root, encoding: "utf8" });
}

test("accepts a small credential-free public history", async (context) => {
  const root = await repository(context);
  await writeFile(path.join(root, "README.md"), "safe public source\n");
  commit(root, "initial");
  const result = runAudit(root);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /verified 1 historical blobs/);
});

test("rejects a forbidden script even after a later deletion commit", async (context) => {
  const root = await repository(context);
  await import("node:fs/promises").then(({ mkdir }) => mkdir(path.join(root, "scripts")));
  const script = path.join(root, "scripts", "publish_github.sh");
  await writeFile(script, "#!/bin/sh\n");
  commit(root, "add private helper");
  await rm(script);
  commit(root, "delete private helper");
  const result = runAudit(root);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /scripts\/publish_github\.sh/);
});

test("rejects credential markers in otherwise allowed historical blobs", async (context) => {
  const root = await repository(context);
  const credentialFixture = ["-----BEGIN", "PRIVATE KEY-----\n"].join(" ");
  await writeFile(path.join(root, "accidental.txt"), credentialFixture);
  commit(root, "accidental credential");
  const result = runAudit(root);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /private credential marker/);
});

test("rejects non-noreply author and committer metadata", async (context) => {
  const root = await repository(context);
  git(root, "config", "user.email", "personal@example.com");
  await writeFile(path.join(root, "README.md"), "safe content with private commit metadata\n");
  commit(root, "private identity");
  const result = runAudit(root);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /non-noreply identity metadata \(1 author commit\(s\), 1 committer commit\(s\)\)/);
  assert.doesNotMatch(result.stderr, /personal@example\.com/);
});
