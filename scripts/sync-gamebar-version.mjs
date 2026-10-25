#!/usr/bin/env node
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

function canonicalJson(value) {
  if (value === null) return "null";
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "number") return JSON.stringify(value);
  if (typeof value === "string") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  return `{${Object.keys(value).sort().map((k) => `${JSON.stringify(k)}:${canonicalJson(value[k])}`).join(",")}}`;
}
function arg(name) {
  const i = process.argv.indexOf(`--${name}`);
  return i >= 0 ? process.argv[i + 1] : undefined;
}
function privateKeyPem() {
  const raw = process.env.YUMMI_AGENT_MANIFEST_SIGNING_KEY?.trim();
  if (!raw) throw new Error("YUMMI_AGENT_MANIFEST_SIGNING_KEY is required");
  return raw.includes("BEGIN") ? raw : Buffer.from(raw, "base64").toString("utf8");
}
const zip = path.resolve(arg("zip") ?? "");
const out = path.resolve(arg("out") ?? "gamebar-version.json");
const version = arg("version");
const releaseTag = arg("release-tag");
const buildId = arg("build-id") ?? process.env.GITHUB_RUN_NUMBER ?? "local";
const commit = arg("commit") ?? process.env.GITHUB_SHA?.slice(0, 7) ?? "unknown";
if (!version || !releaseTag || !fs.existsSync(zip)) throw new Error("version, release-tag and zip are required");
const sha256 = crypto.createHash("sha256").update(fs.readFileSync(zip)).digest("hex");
const gamebar = {
  version,
  releaseTag,
  buildId,
  commit,
  url: `https://github.com/Waitence7/YummiLCU/releases/download/${releaseTag}/${path.basename(zip)}`,
  sha256,
  size: fs.statSync(zip).size,
};
const key = privateKeyPem();
gamebar.signature = crypto.sign(null, Buffer.from(canonicalJson(gamebar), "utf8"), key).toString("base64");
fs.writeFileSync(out, `${JSON.stringify({ schemaVersion: 1, gamebar }, null, 2)}\n`);
console.log(`Wrote ${out}`);
console.log(`sha256=${sha256}`);
