import { execFileSync } from "node:child_process";

const MAX_GITHUB_BLOB_BYTES = 100_000_000;
const forbiddenPath = /(^|\/)(?:scripts|private)(?:\/|$)|(^|\/)(?:AGENTS\.md|id_rsa|id_ed25519)$|\.(?:pem|p12|pfx)$/i;
const credentialMarkers = [
  /BEGIN (?:RSA |EC |OPENSSH |PRIVATE )?PRIVATE KEY/,
  /ghp_[A-Za-z0-9]{20,}/,
  /github_pat_[A-Za-z0-9_]{20,}/,
  /AKIA[0-9A-Z]{16}/,
];

function git(args, options = {}, input) {
  return execFileSync("git", args, {
    encoding: options.encoding ?? "utf8",
    input,
    maxBuffer: 64 * 1024 * 1024,
  });
}

const objects = git(["rev-list", "--objects", "--all"])
  .trim()
  .split("\n")
  .filter(Boolean)
  .map((line) => ({ object: line.slice(0, 40), path: line.length > 41 ? line.slice(41) : "" }));

function isForbiddenPath(entry) {
  if (forbiddenPath.test(entry)) return true;
  const base = entry.split("/").at(-1);
  return base === ".env" || (base?.startsWith(".env.") && base !== ".env.example");
}

const forbidden = [...new Set(objects.map(({ path }) => path).filter((entry) => entry && isForbiddenPath(entry)))].sort();
if (forbidden.length > 0) {
  throw new Error(`public Git history contains forbidden paths:\n${forbidden.join("\n")}`);
}

const ids = [...new Set(objects.map(({ object }) => object))];
const metadata = git(
  ["cat-file", "--batch-check=%(objectname) %(objecttype) %(objectsize)"],
  { encoding: "utf8" },
  ids.join("\n"),
);

let blobCount = 0;
for (const line of metadata.trim().split("\n")) {
  const [object, type, rawSize] = line.split(" ");
  if (type !== "blob") continue;
  blobCount += 1;
  const size = Number(rawSize);
  if (!Number.isSafeInteger(size) || size < 0) throw new Error(`invalid Git object size for ${object}`);
  if (size >= MAX_GITHUB_BLOB_BYTES) throw new Error(`Git history blob ${object} is ${size} bytes`);
  const bytes = execFileSync("git", ["cat-file", "blob", object], {
    encoding: "buffer",
    maxBuffer: MAX_GITHUB_BLOB_BYTES,
  });
  const text = bytes.toString("latin1");
  if (credentialMarkers.some((pattern) => pattern.test(text))) {
    throw new Error(`Git history blob ${object} contains a private credential marker`);
  }
}

const githubNoreply = /^(?:[0-9]+\+)?[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?@users\.noreply\.github\.com$/i;
const identities = git(["log", "--all", "--format=%ae%x00%ce"])
  .trim()
  .split("\n")
  .filter(Boolean)
  .map((line) => line.split("\0"));
const nonNoreplyAuthors = identities.filter(([author]) => !githubNoreply.test(author));
const nonNoreplyCommitters = identities.filter(([, committer]) => !githubNoreply.test(committer));
if (nonNoreplyAuthors.length > 0 || nonNoreplyCommitters.length > 0) {
  throw new Error(`public Git history contains non-noreply identity metadata (${nonNoreplyAuthors.length} author commit(s), ${nonNoreplyCommitters.length} committer commit(s))`);
}
process.stdout.write(`verified ${blobCount} historical blobs below 100 MB with no forbidden path or credential marker\n`);
