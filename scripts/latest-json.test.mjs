import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { buildLatestJson, collectRelease } from "./latest-json.mjs";
import { decodeTauriKey } from "./publish-release.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

test("latest.json lists signed platform urls", () => {
  const body = buildLatestJson({
    version: "0.1.0-dev.12",
    notes: "commit abcdef0",
    pubDate: "2026-11-12T00:00:00.000Z",
    platforms: [
      {
        platform: "darwin-aarch64",
        url: "https://github.com/kurlez/newtype-releases/releases/download/dev/newtype.app.tar.gz",
        signature: "c2ln\n",
      },
    ],
  });
  assert.equal(body.version, "0.1.0-dev.12");
  assert.equal(body.platforms["darwin-aarch64"].signature, "c2ln");
  assert.match(body.platforms["darwin-aarch64"].url, /\/download\/dev\/newtype\.app\.tar\.gz$/);
});

test("collectRelease keeps the dmg out of the manifest and requires a signature", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "newtype-release-"));
  try {
    const mac = path.join(dir, "macos");
    const win = path.join(dir, "windows");
    mkdirSync(mac);
    mkdirSync(win);
    const meta = {
      version: "0.1.0-dev.12",
      commit: "abcdef0",
      fullVersion: "0.1.0-dev.12+abcdef0",
      channel: "dev",
    };
    writeFileSync(
      path.join(mac, "build-meta.json"),
      JSON.stringify({ ...meta, updaterPlatform: "darwin-aarch64" }),
    );
    writeFileSync(
      path.join(win, "build-meta.json"),
      JSON.stringify({ ...meta, updaterPlatform: "windows-x86_64" }),
    );
    writeFileSync(path.join(mac, "newtype.app.tar.gz"), "mac-bytes");
    writeFileSync(path.join(mac, "newtype.app.tar.gz.sig"), "bWFjLXNpZw==\n");
    writeFileSync(path.join(mac, "newtype_0.1.0-dev.12_aarch64.dmg"), "dmg");
    writeFileSync(path.join(win, "newtype_0.1.0-dev.12_x64-setup.exe"), "exe");
    writeFileSync(path.join(win, "newtype_0.1.0-dev.12_x64-setup.exe.sig"), "ZXhlLXNpZw==");
    const plan = collectRelease(dir, { repo: "kurlez/newtype-releases", tag: "dev" });
    assert.equal(plan.tag, "dev");
    assert.equal(plan.prerelease, true);
    assert.equal(plan.latest.version, "0.1.0-dev.12");
    assert.equal(Object.keys(plan.latest.platforms).sort().join(","), "darwin-aarch64,windows-x86_64");
    assert.equal(plan.latest.platforms["windows-x86_64"].signature, "ZXhlLXNpZw==");
    assert.equal(
      plan.latest.platforms["darwin-aarch64"].url,
      "https://github.com/kurlez/newtype-releases/releases/download/dev/newtype.app.tar.gz",
    );
    assert.ok(plan.uploadFiles.some((file) => file.endsWith(".dmg")));
    assert.equal(
      plan.latest.platforms["darwin-aarch64"].url.includes(".dmg"),
      false,
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("stable channel publishes on the version tag", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "newtype-stable-"));
  try {
    writeFileSync(
      path.join(dir, "build-meta.json"),
      JSON.stringify({
        version: "0.1.0",
        commit: "abcdef0",
        channel: "stable",
        updaterPlatform: "darwin-aarch64",
      }),
    );
    writeFileSync(path.join(dir, "newtype.app.tar.gz"), "bytes");
    writeFileSync(path.join(dir, "newtype.app.tar.gz.sig"), "c2ln");
    const plan = collectRelease(dir, { repo: "kurlez/newtype-releases", tag: "dev" });
    assert.equal(plan.tag, "v0.1.0");
    assert.equal(plan.prerelease, false);
    assert.match(plan.latest.platforms["darwin-aarch64"].url, /\/download\/v0\.1\.0\//);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("the embedded updater pubkey is real and is not the placeholder", () => {
  const placeholder = readFileSync(
    path.join(root, "src-tauri/placeholder-updater-pubkey.txt"),
    "utf8",
  ).trim();
  const conf = JSON.parse(readFileSync(path.join(root, "src-tauri/tauri.conf.json"), "utf8"));
  const pubkey = conf.plugins.updater.pubkey.trim();
  assert.notEqual(pubkey, placeholder);
  assert.match(decodeTauriKey(pubkey), /minisign public key/);
  assert.match(decodeTauriKey(placeholder), /minisign public key/);
});
