import assert from "node:assert/strict";
import test from "node:test";
import { appleSigningIdentity, childBuildEnv, compareVersions, configOverlay, nextDevBase, resolveVersion, updaterPlatform } from "./with-version.mjs";

test("local builds use dev.0 and a display commit", () => {
  const resolved = resolveVersion({
    base: "0.1.0",
    ref: "",
    runNumber: "",
    sha: "ABCDEF0123456789",
    platform: "darwin",
    arch: "arm64",
  });
  assert.equal(resolved.version, "0.1.1-dev.0");
  assert.equal(resolved.commit, "abcdef0");
  assert.equal(resolved.fullVersion, "0.1.1-dev.0+abcdef0");
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
  assert.equal(resolved.version, "0.1.1-dev.12");
  assert.equal(resolved.fullVersion, "0.1.1-dev.12+1234567");
  assert.equal(resolved.channel, "dev");
  assert.equal(resolved.updaterPlatform, "windows-x86_64");
});

test("dev build numbers stay in semver prerelease order", () => {
  const a = resolveVersion({ base: "0.1.0", runNumber: "2", sha: "aaaaaaa" });
  const b = resolveVersion({ base: "0.1.0", runNumber: "10", sha: "bbbbbbb" });
  assert.equal(a.version, "0.1.1-dev.2");
  assert.equal(b.version, "0.1.1-dev.10");
  assert.ok(compareVersions(a.version, b.version) < 0);
  assert.ok(compareVersions(b.version, "0.1.1") < 0);
  assert.equal(nextDevBase("0.2.0"), "0.2.1");
  const stable = resolveVersion({
    base: "0.2.0",
    ref: "refs/tags/v0.2.0",
    runNumber: "90",
    sha: "abc1234ffff",
  });
  const laterDev = resolveVersion({ base: "0.2.0", runNumber: "91", sha: "bbbbbbb" });
  assert.equal(stable.version, "0.2.0");
  assert.equal(laterDev.version, "0.2.1-dev.91");
  assert.ok(compareVersions("0.1.0-dev.89", "0.2.0") < 0);
  assert.ok(compareVersions("0.2.0", laterDev.version) < 0);
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

test("missing Apple secrets are not forwarded as empty certificate env", () => {
  const env = {
    APPLE_CERTIFICATE: "",
    APPLE_CERTIFICATE_PASSWORD: "   ",
    APPLE_SIGNING_IDENTITY: "",
    KEEP: "yes",
  };
  assert.equal(appleSigningIdentity(env), "");
  assert.equal("APPLE_CERTIFICATE" in env, false);
  assert.equal("APPLE_CERTIFICATE_PASSWORD" in env, false);
  assert.equal(env.KEEP, "yes");
  assert.throws(() =>
    appleSigningIdentity({
      APPLE_CERTIFICATE: "cert",
      APPLE_CERTIFICATE_PASSWORD: "",
      APPLE_SIGNING_IDENTITY: "newtype-self-signed",
    }),
  );
});

test("stable signing identity is a config overlay and the p12 is not passed to Tauri", () => {
  const overlay = configOverlay("0.1.0-dev.4", "newtype-self-signed");
  assert.equal(overlay.bundle.macOS.signingIdentity, "newtype-self-signed");
  assert.equal(overlay.bundle.macOS.hardenedRuntime, false);
  assert.deepEqual(configOverlay("0.1.0-dev.4", ""), { version: "0.1.0-dev.4" });
  const full = {
    APPLE_CERTIFICATE: "cert",
    APPLE_CERTIFICATE_PASSWORD: "secret",
    APPLE_SIGNING_IDENTITY: "newtype-self-signed",
    PATH: "/usr/bin",
  };
  assert.equal(appleSigningIdentity({ ...full }), "newtype-self-signed");
  const child = childBuildEnv(full, "newtype-self-signed");
  assert.equal("APPLE_CERTIFICATE" in child, false);
  assert.equal("APPLE_CERTIFICATE_PASSWORD" in child, false);
  assert.equal(child.APPLE_SIGNING_IDENTITY, "newtype-self-signed");
  assert.equal(child.NEWTYPE_MAC_SIGN_IDENTITY, "newtype-self-signed");
  assert.equal(child.PATH, "/usr/bin");
  assert.equal(appleSigningIdentity({ APPLE_SIGNING_IDENTITY: "newtype-self-signed" }), "newtype-self-signed");
  const adHoc = childBuildEnv({ APPLE_SIGNING_IDENTITY: "newtype-self-signed" }, "");
  assert.equal("APPLE_SIGNING_IDENTITY" in adHoc, false);
  assert.equal("NEWTYPE_MAC_SIGN_IDENTITY" in adHoc, false);
});

test("only the two shipping targets have updater platform ids", () => {
  assert.equal(updaterPlatform("darwin", "arm64"), "darwin-aarch64");
  assert.equal(updaterPlatform("win32", "x64"), "windows-x86_64");
  assert.equal(updaterPlatform("linux", "x64"), null);
});

