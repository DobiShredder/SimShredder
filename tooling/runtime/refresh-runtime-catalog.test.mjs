import assert from "node:assert/strict";
import test from "node:test";

import { generateKeyPairSync, sign } from "node:crypto";

import { buildPayload, catalogMatchesDiscovery, decideRefresh, highestCatalog, parseNightlyListing, verifyCatalogSignatures } from "./refresh-runtime-catalog.mjs";

const listing = `
<a href="simc-1210.01.abcdef1-winarm64.7z">unsupported</a>
<a href="simc-1210.01.abcdef1-win64.7z">Windows</a>
<a href="simc-1210-01-macos-abcdef1.dmg">macOS</a>`;

function catalog(sequence, discovered = parseNightlyListing(listing)) {
  return { payload: { sequence, manifests: discovered.map((entry) => ({ ...entry, size: 10, sha256: "a".repeat(64) })) }, signatures: [] };
}

test("selects only a same-revision macOS ARM64 and Windows x64 pair", () => {
  const result = parseNightlyListing(listing);
  assert.deepEqual(result.map(({ platform, architecture, build }) => ({ platform, architecture, build })), [
    { platform: "macos", architecture: "aarch64", build: "abcdef1" },
    { platform: "windows", architecture: "x86_64", build: "abcdef1" },
  ]);
  assert.throws(() => parseNightlyListing(listing.replace("macos-abcdef1", "macos-bcdef12")), /disagree/);
  assert.throws(() => parseNightlyListing("<a href='simc-1210.01.abcdef1-winarm64.7z'>ARM</a>"), /no complete/);
  assert.throws(() => parseNightlyListing(""), /non-empty/);
});

test("detects unchanged identity and rejects same-sequence substitution", () => {
  const discovered = parseNightlyListing(listing);
  assert.equal(catalogMatchesDiscovery(catalog(6, discovered), discovered), true);
  assert.equal(catalogMatchesDiscovery(catalog(6, discovered), parseNightlyListing(listing.replaceAll("abcdef1", "bcdef12"))), false);
  assert.throws(() => highestCatalog([
    catalog(7, discovered),
    { ...catalog(7, discovered), payload: { ...catalog(7, discovered).payload, generated_at_unix_seconds: 1 } },
  ]), /conflicting payloads/);
});

test("unchanged build performs only availability checks and does not request a refresh", async () => {
  const discovered = parseNightlyListing(listing);
  let checks = 0;
  const unchanged = await decideRefresh([catalog(9, discovered)], discovered, async () => {
    checks += 1;
    return true;
  });
  assert.equal(unchanged.changed, false);
  assert.equal(checks, 1);

  const changedDiscovery = parseNightlyListing(listing.replaceAll("abcdef1", "bcdef12"));
  const changed = await decideRefresh([catalog(9, discovered)], changedDiscovery, async () => {
    throw new Error("availability must not be checked for a different revision");
  });
  assert.equal(changed.changed, true);
});

test("builds a deterministic monotonic payload with a short expiry", () => {
  const discovered = parseNightlyListing(listing);
  const artifacts = discovered.map((entry, index) => ({ ...entry, size: 100 + index, sha256: String(index + 1).repeat(64) }));
  const input = { catalogs: [catalog(8, discovered), catalog(11, discovered)], discovered, artifacts, generatedAt: 1_800_000_000, expiresAt: 1_800_604_800 };
  const first = buildPayload(input);
  const second = buildPayload(input);
  assert.deepEqual(first, second);
  assert.equal(first.sequence, 12);
  assert.equal(first.expires_at_unix_seconds - first.generated_at_unix_seconds, 7 * 24 * 60 * 60);
  assert.throws(() => buildPayload({ ...input, expiresAt: input.generatedAt + 15 * 24 * 60 * 60 }), /no longer than 14 days/);
});

test("verifies Ed25519 signatures and rejects expiry or substitution", () => {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const payload = {
    schema_version: 1,
    sequence: 1,
    generated_at_unix_seconds: 1_799_999_900,
    expires_at_unix_seconds: 1_800_000_100,
    manifests: [],
    next_keys: [],
    revoked_key_ids: [],
  };
  const message = Buffer.concat([Buffer.from("SimShredder runtime catalog v1\0"), Buffer.from(JSON.stringify(payload))]);
  const catalogValue = { payload, signatures: [{ key_id: "fixture", algorithm: "ed25519", signature_base64: sign(null, message, privateKey).toString("base64") }] };
  const der = publicKey.export({ format: "der", type: "spki" });
  const roots = [{ key_id: "fixture", algorithm: "ed25519", public_key_base64: der.subarray(-32).toString("base64") }];
  assert.equal(verifyCatalogSignatures(catalogValue, roots, 1_800_000_000), catalogValue);
  assert.throws(() => verifyCatalogSignatures({ ...catalogValue, payload: { ...payload, sequence: 2 } }, roots, 1_800_000_000), /no valid signature/);
  assert.throws(() => verifyCatalogSignatures(catalogValue, roots, 1_800_000_101), /expired/);
});
