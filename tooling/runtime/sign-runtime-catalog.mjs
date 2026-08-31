import { createPrivateKey, sign } from "node:crypto";
import { open, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const MAX_PAYLOAD_BYTES = 1024 * 1024;
const SIGNATURE_DOMAIN = Buffer.from("SimShredder runtime catalog v1\0");

function fail(message) {
  throw new Error(message);
}

async function readBoundedRegularFile(filename, maximumBytes, label) {
  const handle = await open(filename, "r");
  try {
    const stat = await handle.stat();
    if (!stat.isFile() || stat.size <= 0 || stat.size > maximumBytes) {
      fail(`${label} must be a non-empty regular file no larger than ${maximumBytes} bytes`);
    }
    return await handle.readFile();
  } finally {
    await handle.close();
  }
}

function validatePayload(payload) {
  if (!payload
    || payload.schema_version !== 1
    || !Number.isSafeInteger(payload.sequence)
    || payload.sequence <= 0
    || !Array.isArray(payload.manifests)
    || !Array.isArray(payload.next_keys)
    || !Array.isArray(payload.revoked_key_ids)) {
    fail("payload is not a runtime catalog v1 payload");
  }
  return payload;
}

export function signCatalogPayload(payload, privateKeyPem, keyId) {
  validatePayload(payload);
  if (!/^[A-Za-z0-9._-]{1,128}$/.test(keyId)) fail("key ID is invalid");
  const canonicalPayload = Buffer.from(JSON.stringify(payload));
  const privateKey = createPrivateKey({ key: privateKeyPem, format: "pem", type: "pkcs8" });
  if (privateKey.asymmetricKeyType !== "ed25519") fail("private key must be Ed25519 PKCS#8 PEM");
  const signature = sign(null, Buffer.concat([SIGNATURE_DOMAIN, canonicalPayload]), privateKey);
  if (signature.length !== 64) fail("Ed25519 signature must be 64 bytes");
  return {
    payload,
    signatures: [{
      key_id: keyId,
      algorithm: "ed25519",
      signature_base64: signature.toString("base64"),
    }],
  };
}

function parseArguments(argv) {
  if (argv.length !== 4) {
    fail("usage: sign-runtime-catalog.mjs <payload.json> <private-key.pem> <key-id> <output.json>");
  }
  return { payloadFile: argv[0], keyFile: argv[1], keyId: argv[2], outputFile: argv[3] };
}

export async function main(argv = process.argv.slice(2)) {
  const options = parseArguments(argv);
  const payloadBytes = await readBoundedRegularFile(options.payloadFile, MAX_PAYLOAD_BYTES, "payload");
  const payload = validatePayload(JSON.parse(payloadBytes.toString("utf8")));
  const privateKeyPem = (await readBoundedRegularFile(options.keyFile, 16 * 1024, "private key")).toString("utf8");
  const catalog = signCatalogPayload(payload, privateKeyPem, options.keyId);
  await writeFile(options.outputFile, `${JSON.stringify(catalog, null, 2)}\n`, { flag: "wx", mode: 0o600 });
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main().catch((error) => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
