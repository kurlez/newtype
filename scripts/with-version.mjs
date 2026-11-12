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
      `src-tauri/tauri.conf.json version must be plain semver like 0.1.0 (the next stable). Found ${JSON.stringify(version)}. Dev builds append -dev.<build>; do not put a prerelease in the file.`,
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

/**
 * Comparable updater version.
 *
 * Dev builds are `{base}-dev.{github.run_number}`. The `+commit` form is display-only:
 * Tauri compares `semver::Version`'s Ord, which orders build metadata lexicographically,
 * and `+` is unsafe in artifact file names. See README.
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
    throw new Error(`base version must look like 0.1.0, found ${base}`);
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
    version = `${base}-dev.${runNumber}`;
    channel = "dev";
  } else {
    version = `${base}-dev.0`;
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
// any present APPLE_CERTIFICATE as a PKCS#12 to import, including an empty one.
export function appleSigningIdentity(env) {
  for (const name of APPLE_ENV) {
    if (typeof env[name] === "string" && env[name].trim() === "") {
      delete env[name];
    }
  }
  const present = APPLE_ENV.filter((name) => env[name]);
  if (present.length !== 0 && present.length !== APPLE_ENV.length) {
    throw new Error(
      "Set all of APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD, and APPLE_SIGNING_IDENTITY, or none of them.",
    );
  }
  return present.length === APPLE_ENV.length ? env.APPLE_SIGNING_IDENTITY : "";
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
    { cwd: root, stdio: "inherit", env: { ...process.env, NEWTYPE_COMMIT: resolved.commit, NEWTYPE_VERSION: resolved.version } },
  );
  child.on("exit", (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal);
      return;
    }
    process.exit(code ?? 1);
  });
}
