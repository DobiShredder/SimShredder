import assert from "node:assert/strict";
import { generateKeyPairSync } from "node:crypto";
import test from "node:test";

import { verifyCatalogSignatures } from "./refresh-runtime-catalog.mjs";
import { signCatalogPayload } from "./sign-runtime-catalog.mjs";

function fixturePayload() {
  return {
    schema_version: 1,
    sequence: 7,
    generated_at_unix_seconds: 1_800_000_000,
    expires_at_unix_seconds: 1_800_604_800,
    manifests: [],
    next_keys: [],
    revoked_key_ids: [],
  };
}

test("signs the canonical catalog domain with an Ed25519 PKCS#8 key", () => {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const catalog = signCatalogPayload(
    fixturePayload(),
    privateKey.export({ format: "pem", type: "pkcs8" }),
    "fixture-key",
  );
  const rawPublicKey = publicKey.export({ format: "der", type: "spki" }).subarray(-32);
  assert.equal(
    verifyCatalogSignatures(catalog, [{
      key_id: "fixture-key",
      algorithm: "ed25519",
      public_key_base64: rawPublicKey.toString("base64"),
    }], 1_800_000_001),
    catalog,
  );
});

test("rejects malformed payloads, key IDs, and non-Ed25519 keys", () => {
  const ed25519 = generateKeyPairSync("ed25519").privateKey.export({ format: "pem", type: "pkcs8" });
  const rsa = generateKeyPairSync("rsa", { modulusLength: 2048 }).privateKey.export({ format: "pem", type: "pkcs8" });
  assert.throws(() => signCatalogPayload({}, ed25519, "fixture"), /runtime catalog v1/);
  assert.throws(() => signCatalogPayload(fixturePayload(), ed25519, "invalid key id"), /key ID/);
  assert.throws(() => signCatalogPayload(fixturePayload(), rsa, "fixture"), /Ed25519/);
});
