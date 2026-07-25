#!/usr/bin/env node
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

function readArgs(argv) {
  const out = {};
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (!arg.startsWith("--")) continue;
    const key = arg.slice(2);
    const next = argv[index + 1];
    if (!next || next.startsWith("--")) {
      out[key] = "true";
    } else {
      out[key] = next;
      index += 1;
    }
  }
  return out;
}

function canonicalJson(value) {
  if (value === null) return "null";
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "number") {
    if (!Number.isFinite(value)) throw new Error("non-finite number");
    return JSON.stringify(value);
  }
  if (typeof value === "string") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`)
      .join(",")}}`;
  }
  throw new Error(`unsupported JSON value: ${typeof value}`);
}

function sha256File(file) {
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

function fileSize(file) {
  return fs.statSync(file).size;
}

function readJsonIfExists(file) {
  if (!fs.existsSync(file)) return {};
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

function signingKeyPem() {
  const raw = process.env.YUMMI_AGENT_MANIFEST_SIGNING_KEY?.trim();
  if (!raw) throw new Error("YUMMI_AGENT_MANIFEST_SIGNING_KEY is required");
  if (raw.includes("BEGIN")) return raw;
  return Buffer.from(raw, "base64").toString("utf8");
}

function publicKeyRawBase64(privateKeyPem) {
  const publicKey = crypto.createPublicKey(privateKeyPem);
  const spki = publicKey.export({ format: "der", type: "spki" });
  return spki.subarray(spki.length - 32).toString("base64");
}

function signPayload(payload, privateKeyPem) {
  return crypto.sign(null, Buffer.from(canonicalJson(payload), "utf8"), privateKeyPem).toString("base64");
}

const args = readArgs(process.argv.slice(2));
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const manifestPath = path.resolve(args.out ?? path.join(root, "agent-version.json"));
const publicUrl = (args["public-url"] ?? process.env.AGENT_PUBLIC_URL ?? "https://yummi.duckdns.org").replace(/\/+$/, "");
const zipPath = path.resolve(args.zip ?? "");
const exePath = path.resolve(args.exe ?? "");
const version =
  args.version ??
  JSON.parse(fs.readFileSync(path.join(root, "agent-tauri", "package.json"), "utf8")).version;
const notes = args.notes ?? process.env.AGENT_RELEASE_NOTES ?? `Tauri ${version}`;
const channel = args.channel ?? "stable";
const rolloutPercent = Number.parseInt(args["rollout-percent"] ?? "100", 10);
const executable = args.executable ?? "yummi-lcu-tauri.exe";

if (!fs.existsSync(zipPath)) throw new Error(`zip not found: ${zipPath}`);
if (!fs.existsSync(exePath)) throw new Error(`exe not found: ${exePath}`);
if (!["stable", "beta", "dev"].includes(channel)) {
  throw new Error("channel must be stable, beta, or dev");
}
if (!Number.isInteger(rolloutPercent) || rolloutPercent < 0 || rolloutPercent > 100) {
  throw new Error("rollout-percent must be 0..100");
}

const privateKey = signingKeyPem();
const derivedPublicKey = publicKeyRawBase64(privateKey);
const configuredPublicKey = process.env.YUMMI_AGENT_MANIFEST_PUBLIC_KEY?.trim();
if (configuredPublicKey && configuredPublicKey !== derivedPublicKey) {
  throw new Error("YUMMI_AGENT_MANIFEST_PUBLIC_KEY does not match signing key");
}

const zipSha256 = sha256File(zipPath);
const exeSha256 = sha256File(exePath);
const tauri = {
  version,
  channel,
  rolloutPercent,
  url: `${publicUrl}/agent/releases/tauri/tauri-${version}.zip`,
  sha256: zipSha256,
  executable,
  notes,
  files: [
    {
      path: executable,
      sha256: exeSha256,
      size: fileSize(exePath),
    },
  ],
};

if (args["min-version"]) tauri.minVersion = args["min-version"];
if (args["blocked-version"]) {
  tauri.blockedVersions = String(args["blocked-version"])
    .split(",")
    .map((value) => value.trim())
    .filter(Boolean);
}
const publisherThumbprint =
  args["publisher-thumbprint"] ?? process.env.YUMMI_AGENT_WINDOWS_SIGNING_THUMBPRINT;
if (publisherThumbprint) {
  tauri.publisherThumbprint = publisherThumbprint.replace(/[^0-9a-f]/gi, "").toLowerCase();
}

tauri.signature = signPayload(tauri, privateKey);

const manifest = readJsonIfExists(manifestPath);
manifest.schemaVersion = 2;
manifest.notes = notes;
manifest.version = version;
manifest.url = tauri.url;
manifest.sha256 = zipSha256;
manifest.tauri = tauri;

fs.mkdirSync(path.dirname(manifestPath), { recursive: true });
fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`Wrote ${manifestPath}`);
console.log(`version=${version}`);
console.log(`sha256=${zipSha256}`);
console.log(`publicKey=${derivedPublicKey}`);
