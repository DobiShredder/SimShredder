import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const localeDirectory = fileURLToPath(
  new URL("../../apps/desktop/src/locales/", import.meta.url),
);
const localeNames = ["en", "ko"];

function flatten(value, path = [], result = new Map()) {
  if (typeof value === "string") {
    const key = path.join(".");
    if (value.trim().length === 0) {
      throw new Error(`${key}: translation must not be empty`);
    }
    result.set(key, value);
    return result;
  }

  if (value === null || Array.isArray(value) || typeof value !== "object") {
    throw new Error(`${path.join(".") || "<root>"}: expected an object or string`);
  }

  for (const [key, child] of Object.entries(value)) {
    if (key.length === 0 || key.includes(".")) {
      throw new Error(`${[...path, key].join(".")}: keys must be non-empty and contain no dots`);
    }
    flatten(child, [...path, key], result);
  }

  return result;
}

function placeholders(message) {
  const names = [];
  for (const match of message.matchAll(/{{\s*([^},\s]+)(?:\s*,[^}]*)?\s*}}/g)) {
    names.push(match[1]);
  }
  return [...new Set(names)].sort();
}

function difference(left, right) {
  return [...left].filter((key) => !right.has(key)).sort();
}

const catalogs = new Map();
for (const locale of localeNames) {
  const source = await readFile(`${localeDirectory}${locale}.json`, "utf8");
  catalogs.set(locale, flatten(JSON.parse(source)));
}

const reference = catalogs.get("en");
const errors = [];

for (const locale of localeNames.filter((name) => name !== "en")) {
  const catalog = catalogs.get(locale);
  const missing = difference(reference.keys(), catalog);
  const extra = difference(catalog.keys(), reference);

  if (missing.length > 0) errors.push(`${locale}: missing keys: ${missing.join(", ")}`);
  if (extra.length > 0) errors.push(`${locale}: extra keys: ${extra.join(", ")}`);

  for (const [key, source] of reference) {
    const translated = catalog.get(key);
    if (translated === undefined) continue;
    const expected = placeholders(source);
    const actual = placeholders(translated);
    if (expected.join("\0") !== actual.join("\0")) {
      errors.push(
        `${locale}:${key}: placeholders must be ${JSON.stringify(expected)}, received ${JSON.stringify(actual)}`,
      );
    }
  }
}

if (errors.length > 0) {
  console.error(`Locale validation failed:\n- ${errors.join("\n- ")}`);
  process.exitCode = 1;
} else {
  console.log(`Locale validation passed: ${localeNames.length} locales, ${reference.size} messages.`);
}
