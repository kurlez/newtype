import { readdirSync, rmSync, statSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const DAY_MS = 24 * 60 * 60 * 1000;

export function decideTarget({ bytes, newestMs, now, maxBytes, maxAgeMs }) {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "keep";
  }
  if (Number.isFinite(newestMs) && now - newestMs > maxAgeMs) {
    return "delete-stale";
  }
  if (bytes > maxBytes) {
    return "delete-over-cap";
  }
  return "keep";
}

function walk(dir) {
  let bytes = 0;
  let newestMs = 0;
  const stack = [dir];
  while (stack.length > 0) {
    const current = stack.pop();
    let entries = [];
    try {
      entries = readdirSync(current, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      const full = path.join(current, entry.name);
      let info;
      try {
        info = statSync(full);
      } catch {
        continue;
      }
      if (info.isSymbolicLink()) {
        continue;
      }
      if (info.isDirectory()) {
        stack.push(full);
        continue;
      }
      bytes += info.size;
      if (info.mtimeMs > newestMs) {
        newestMs = info.mtimeMs;
      }
    }
  }
  return { bytes, newestMs };
}

function removeTree(dir) {
  rmSync(dir, { recursive: true, force: true });
}

function deleteOlderFiles(dir, cutoffMs) {
  const stack = [dir];
  while (stack.length > 0) {
    const current = stack.pop();
    let entries = [];
    try {
      entries = readdirSync(current, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      const full = path.join(current, entry.name);
      let info;
      try {
        info = statSync(full);
      } catch {
        continue;
      }
      if (info.isSymbolicLink()) {
        continue;
      }
      if (info.isDirectory()) {
        stack.push(full);
        continue;
      }
      if (info.mtimeMs < cutoffMs) {
        rmSync(full, { force: true });
      }
    }
  }
}

function pruneTarget(dir, { now, maxBytes, maxAgeMs }) {
  const measured = walk(dir);
  const action = decideTarget({ ...measured, now, maxBytes, maxAgeMs });
  if (action !== "keep") {
    removeTree(dir);
  }
  console.log(
    `cargo target ${dir}: ${measured.bytes} bytes, newest ${measured.newestMs ? new Date(measured.newestMs).toISOString() : "empty"} -> ${action}`,
  );
}

export function planPackagePrune({ hasManifest, dirMtimeMs, cutoffMs, dropBroken }) {
  if (dropBroken && !hasManifest) {
    return "delete-broken";
  }
  if (Number.isFinite(dirMtimeMs) && dirMtimeMs < cutoffMs) {
    return "delete-stale";
  }
  return "keep";
}

function childDirs(dir) {
  let entries = [];
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return [];
  }
  return entries.filter((entry) => entry.isDirectory()).map((entry) => path.join(dir, entry.name));
}

function prunePackageDirs(root, cutoffMs, dropBroken) {
  for (const indexDir of childDirs(root)) {
    for (const pkg of childDirs(indexDir)) {
      let info;
      try {
        info = statSync(pkg);
      } catch {
        continue;
      }
      let hasManifest = false;
      try {
        hasManifest = statSync(path.join(pkg, "Cargo.toml")).isFile();
      } catch {
        hasManifest = false;
      }
      const action = planPackagePrune({
        hasManifest,
        dirMtimeMs: info.mtimeMs,
        cutoffMs,
        dropBroken,
      });
      if (action !== "keep") {
        rmSync(pkg, { recursive: true, force: true });
      }
    }
  }
  console.log(`pruned package directories under ${root}`);
}

function main() {
  const days = Number(process.env.NEWTYPE_CACHE_DAYS || 14);
  const maxBytes = Number(process.env.NEWTYPE_TARGET_CAP_BYTES || 8 * 1024 * 1024 * 1024);
  const now = Date.now();
  const maxAgeMs = days * DAY_MS;
  const target = process.env.CARGO_TARGET_DIR || path.join(os.homedir(), ".cache", "newtype", "cargo-target");
  const cargoHome = process.env.CARGO_HOME || path.join(os.homedir(), ".cargo");
  pruneTarget(target, { now, maxBytes, maxAgeMs });
  const cutoffMs = now - maxAgeMs;
  prunePackageDirs(path.join(cargoHome, "registry", "src"), cutoffMs, true);
  deleteOlderFiles(path.join(cargoHome, "registry", "cache"), cutoffMs);
  console.log(`pruned files older than the cutoff under ${path.join(cargoHome, "registry", "cache")}`);
  prunePackageDirs(path.join(cargoHome, "git", "checkouts"), cutoffMs, false);
  const npmCache = path.join(os.homedir(), ".npm");
  const npm = walk(npmCache);
  if (npm.bytes > 2 * 1024 * 1024 * 1024) {
    deleteOlderFiles(npmCache, now - maxAgeMs);
    console.log(`npm cache was ${npm.bytes} bytes; removed entries older than ${days} days`);
  } else {
    console.log(`npm cache ${npm.bytes} bytes, left in place`);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
