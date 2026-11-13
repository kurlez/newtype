import { spawn } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const BASE_VERSION = /^\d+\.\d+\.\d+$/;
const TAG_VERSION = /^v(\d+\.\d+\.\d+)$/;

export function readBaseVersion(confPath = path.join(root, "src-tauri/tauri.conf.json")) {
  const conf = JSON.parse(readFileSync(confPath, "utf8"));
  const version = conf.version;
  if (typeof version !== "string" || !BASE_VERSION.test(version)) {
    throw new Error(
      `src-tauri/tauri.conf.json version must be plain semver like 0.2.0 (the latest stable). Found ${JSON.stringify(version)}. Dev builds use the next patch, as 0.2.1-dev.<build>. Do not put a prerelease in the file.`,
    );
  }
  return version;
}

export function updaterPlatform(platform, arch) {
  if (platform === "darwin" && arch === "arm64") {
    return "darwin-aarch64";
  }
  if (platform === "win32" && arch === "x64") {
    return "windows-x86_64";
  }
  return null;
}

export function shortCommit(sha) {
  const trimmed = (sha ?? "").trim();
  if (/^[0-9a-f]{7,40}$/i.test(trimmed)) {
    return trimmed.slice(0, 7).toLowerCase();
  }
  return "unknown";
}

/** The next patch after the stable version in tauri.conf.json. `0.2.0` becomes `0.2.1`. */
export function nextDevBase(base) {
  if (!BASE_VERSION.test(base)) {
    throw new Error(`base version must look like 0.2.0, found ${base}`);
  }
  const [major, minor, patch] = base.split(".").map((part) => Number(part));
  return `${major}.${minor}.${patch + 1}`;
}

/**
 * Semver order used by the updater, without build metadata.
 * A release sorts above its own prereleases: `0.2.0-dev.9 < 0.2.0 < 0.2.1-dev.1`.
 */
export function compareVersions(left, right) {
  const a = parseSemver(left);
  const b = parseSemver(right);
  for (let index = 0; index < 3; index += 1) {
    if (a.nums[index] !== b.nums[index]) {
      return a.nums[index] < b.nums[index] ? -1 : 1;
    }
  }
  if (a.pre.length === 0 && b.pre.length === 0) {
    return 0;
  }
  if (a.pre.length === 0) {
    return 1;
  }
  if (b.pre.length === 0) {
    return -1;
  }
  const count = Math.max(a.pre.length, b.pre.length);
  for (let index = 0; index < count; index += 1) {
    if (a.pre[index] === undefined) {
      return -1;
    }
    if (b.pre[index] === undefined) {
      return 1;
    }
    const aNumeric = /^\d+$/.test(a.pre[index]);
    const bNumeric = /^\d+$/.test(b.pre[index]);
    if (aNumeric && bNumeric && a.pre[index] !== b.pre[index]) {
      return Number(a.pre[index]) < Number(b.pre[index]) ? -1 : 1;
    }
    if (aNumeric !== bNumeric) {
      return aNumeric ? -1 : 1;
    }
    if (a.pre[index] !== b.pre[index]) {
      return a.pre[index] < b.pre[index] ? -1 : 1;
    }
  }
  return 0;
}

function parseSemver(version) {
  const bare = String(version).split("+")[0];
  const [core, pre = ""] = bare.split("-");
  const nums = core.split(".").map((part) => Number(part));
  while (nums.length < 3) {
    nums.push(0);
  }
  return { nums, pre: pre ? pre.split(".") : [] };
}

/**
 * Comparable updater version.
 *
 * The file version is the latest stable release. Dev builds are the next patch,
 * `{next}-dev.{github.run_number}`, so they sort above that stable release.
 * A `v*` tag uses the tag number and ignores the file. The `+commit` form is
 * display-only: Tauri compares `semver::Version`'s Ord, which orders build
 * metadata lexicographically, and `+` is unsafe in artifact file names.
 */
export function resolveVersion({
  base,
  ref = "",
  runNumber = "",
  sha = "",
  platform = process.platform,
  arch = process.arch,
}) {
  if (!BASE_VERSION.test(base)) {
    throw new Error(`base version must look like 0.2.0, found ${base}`);
  }
  const commit = shortCommit(sha);
  let version;
  let channel;
  if (ref.startsWith("refs/tags/")) {
    const name = ref.slice("refs/tags/".length);
    const tag = TAG_VERSION.exec(name);
    if (!tag) {
      throw new Error(
        `Tag ${name} must look like v1.2.3 with no prerelease. Dev builds are published from main, not from tags.`,
      );
    }
    version = tag[1];
    channel = "stable";
  } else if (runNumber !== "" && runNumber !== undefined && runNumber !== null) {
    if (!/^[1-9]\d*$/.test(String(runNumber))) {
      throw new Error(`GITHUB_RUN_NUMBER must be a positive integer, found ${runNumber}`);
    }
    version = `${nextDevBase(base)}-dev.${runNumber}`;
    channel = "dev";
  } else {
    version = `${nextDevBase(base)}-dev.0`;
    channel = "dev";
  }
  return {
    version,
    commit,
    fullVersion: `${version}+${commit}`,
    channel,
    updaterPlatform: updaterPlatform(platform, arch),
  };
}

const APPLE_ENV = [
  "APPLE_CERTIFICATE",
  "APPLE_CERTIFICATE_PASSWORD",
  "APPLE_SIGNING_IDENTITY",
];

// GitHub Actions sets an env var to "" when the secret is missing. Tauri treats
// any present APPLE_CERTIFICATE as a Developer ID PKCS#12, including an empty
// one, and its importer only accepts Apple certificate name prefixes. A
// self-signed identity is imported by scripts/import-macos-identity.sh. This
// function then keeps the certificate name and the child build env drops the
// PKCS#12 so Tauri calls codesign with that name.
export function appleSigningIdentity(env) {
  for (const name of APPLE_ENV) {
    if (typeof env[name] === "string" && env[name].trim() === "") {
      delete env[name];
    }
  }
  const identity = typeof env.APPLE_SIGNING_IDENTITY === "string" ? env.APPLE_SIGNING_IDENTITY.trim() : "";
  const cert = Boolean(env.APPLE_CERTIFICATE);
  const password = Boolean(env.APPLE_CERTIFICATE_PASSWORD);
  if (!identity && !cert && !password) {
    return "";
  }
  if (identity && ((cert && password) || (!cert && !password))) {
    return identity;
  }
  throw new Error(
    "Set all of APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD, and APPLE_SIGNING_IDENTITY, or none of them. After the keychain import, APPLE_SIGNING_IDENTITY alone is enough.",
  );
}

export function childBuildEnv(env, identity) {
  const child = { ...env };
  delete child.APPLE_CERTIFICATE;
  delete child.APPLE_CERTIFICATE_PASSWORD;
  if (identity) {
    child.APPLE_SIGNING_IDENTITY = identity;
    child.NEWTYPE_MAC_SIGN_IDENTITY = identity;
  } else {
    delete child.APPLE_SIGNING_IDENTITY;
    delete child.NEWTYPE_MAC_SIGN_IDENTITY;
  }
  return child;
}

export function configOverlay(version, signingIdentity) {
  const overlay = { version };
  if (signingIdentity) {
    overlay.bundle = {
      macOS: {
        signingIdentity,
        hardenedRuntime: false,
      },
    };
  }
  return overlay;
}

function gitShortSha() {
  try {
    const { execFileSync } = awaitImport();
    return execFileSync("git", ["rev-parse", "HEAD"], {
      cwd: root,
      encoding: "utf8",
    });
  } catch {
    return "";
  }
}

function awaitImport() {
  return { execFileSync: globalThis.__newtypeExecFileSync };
}

function isDirectRun() {
  const entry = process.argv[1];
  if (!entry) {
    return false;
  }
  return path.resolve(entry) === fileURLToPath(import.meta.url);
}

if (isDirectRun()) {
  const { execFileSync } = await import("node:child_process");
  globalThis.__newtypeExecFileSync = execFileSync;
  const separator = process.argv.indexOf("--");
  if (separator === -1 || separator === process.argv.length - 1) {
    console.error("usage: node scripts/with-version.mjs -- <tauri args>");
    process.exit(1);
  }
  const tauriArgs = process.argv.slice(separator + 1);
  const sha = process.env.GITHUB_SHA || gitShortSha();
  const resolved = resolveVersion({
    base: readBaseVersion(),
    ref: process.env.GITHUB_REF ?? "",
    runNumber: process.env.GITHUB_RUN_NUMBER ?? "",
    sha,
  });
  let signingIdentity = "";
  try {
    signingIdentity = appleSigningIdentity(process.env);
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  }
  const overlay = configOverlay(
    resolved.version,
    process.platform === "darwin" ? signingIdentity : "",
  );
  const childEnv = childBuildEnv(process.env, process.platform === "darwin" ? signingIdentity : "");
  const metaPath = path.join(root, "build-meta.json");
  writeFileSync(metaPath, `${JSON.stringify(resolved, null, 2)}\n`);
  console.log(
    `newtype ${resolved.fullVersion} channel=${resolved.channel} updater=${resolved.updaterPlatform}`,
  );
  if (signingIdentity) {
    console.log("macOS code signature: stable identity from APPLE_SIGNING_IDENTITY");
  } else if (process.platform === "darwin") {
    console.log('macOS code signature: ad-hoc (signingIdentity "-")');
  }
  const cli = path.join(root, "node_modules/@tauri-apps/cli/tauri.js");
  const child = spawn(
    process.execPath,
    [cli, ...tauriArgs, "--config", JSON.stringify(overlay)],
    {
      cwd: root,
      stdio: "inherit",
      env: { ...childEnv, NEWTYPE_COMMIT: resolved.commit, NEWTYPE_VERSION: resolved.version },
    },
  );
  child.on("exit", (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal);
      return;
    }
    process.exit(code ?? 1);
  });
}
