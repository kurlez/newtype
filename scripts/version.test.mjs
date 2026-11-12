import assert from "node:assert/strict";
import test from "node:test";
import { configOverlay, resolveVersion, updaterPlatform } from "./with-version.mjs";

test("local builds use dev.0 and a display commit", () => {
  const resolved = resolveVersion({
    base: "0.1.0",
    ref: "",
    runNumber: "",
    sha: "ABCDEF0123456789",
    platform: "darwin",
    arch: "arm64",
  });
  assert.equal(resolved.version, "0.1.0-dev.0");
  assert.equal(resolved.commit, "abcdef0");
  assert.equal(resolved.fullVersion, "0.1.0-dev.0+abcdef0");
  assert.equal(resolved.channel, "dev");
  assert.equal(resolved.updaterPlatform, "darwin-aarch64");
});

test("main builds use the workflow run number", () => {
  const resolved = resolveVersion({
    base: "0.1.0",
    ref: "refs/heads/main",
    runNumber: "12",
    sha: "1234567890abcdef",
    platform: "win32",
    arch: "x64",
  });
  assert.equal(resolved.version, "0.1.0-dev.12");
  assert.equal(resolved.fullVersion, "0.1.0-dev.12+1234567");
  assert.equal(resolved.channel, "dev");
  assert.equal(resolved.updaterPlatform, "windows-x86_64");
});

test("dev build numbers stay in semver prerelease order", () => {
  const a = resolveVersion({ base: "0.1.0", runNumber: "2", sha: "aaaaaaa" });
  const b = resolveVersion({ base: "0.1.0", runNumber: "10", sha: "bbbbbbb" });
  assert.equal(a.version, "0.1.0-dev.2");
  assert.equal(b.version, "0.1.0-dev.10");
  assert.ok(comparePrerelease(a.version, b.version) < 0);
  assert.ok(comparePrerelease(b.version, "0.1.0") < 0);
  assert.ok(comparePrerelease("0.1.0", "0.2.0-dev.1") < 0);
});

test("a v* tag is the stable version and ignores the run number", () => {
  const resolved = resolveVersion({
    base: "0.1.0",
    ref: "refs/tags/v0.1.0",
    runNumber: "99",
    sha: "abc1234ffff",
    platform: "darwin",
    arch: "arm64",
  });
  assert.equal(resolved.version, "0.1.0");
  assert.equal(resolved.channel, "stable");
});

test("rejects prerelease tags and bad run numbers", () => {
  assert.throws(() => resolveVersion({ base: "0.1.0", ref: "refs/tags/v0.1.0-dev.1" }));
  assert.throws(() => resolveVersion({ base: "0.1.0", runNumber: "01" }));
  assert.throws(() => resolveVersion({ base: "0.1.0-dev.1", runNumber: "3" }));
});

test("stable signing identity is a config overlay", () => {
  const overlay = configOverlay("0.1.0-dev.4", "newtype-self-signed");
  assert.equal(overlay.bundle.macOS.signingIdentity, "newtype-self-signed");
  assert.equal(overlay.bundle.macOS.hardenedRuntime, false);
  assert.deepEqual(configOverlay("0.1.0-dev.4", ""), { version: "0.1.0-dev.4" });
});

test("only the two shipping targets have updater platform ids", () => {
  assert.equal(updaterPlatform("darwin", "arm64"), "darwin-aarch64");
  assert.equal(updaterPlatform("win32", "x64"), "windows-x86_64");
  assert.equal(updaterPlatform("linux", "x64"), null);
});

function comparePrerelease(left, right) {
  const a = split(left);
  const b = split(right);
  if (a.core !== b.core) {
    return a.core < b.core ? -1 : 1;
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
    const an = /^\d+$/.test(a.pre[index]);
    const bn = /^\d+$/.test(b.pre[index]);
    if (an && bn && a.pre[index] !== b.pre[index]) {
      return Number(a.pre[index]) < Number(b.pre[index]) ? -1 : 1;
    }
    if (a.pre[index] !== b.pre[index]) {
      return a.pre[index] < b.pre[index] ? -1 : 1;
    }
  }
  return 0;
}

function split(version) {
  const [core, pre = ""] = version.split("-");
  return { core, pre: pre ? pre.split(".") : [] };
}
