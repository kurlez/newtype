import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { applyLanguage, mixesLanguages } from "../src/i18n.ts";
import { routeBadge, statusLine } from "../src/indicator-layout.ts";

const zhWords = { auto: "自动", offline: "离线", local: "本地", cloud: "云端" };
const enWords = { auto: "Auto", offline: "Offline", local: "Local", cloud: "Cloud" };

test("the overlay is about a quarter of the previous 480 by 132 panel", () => {
  const overlay = readFileSync(new URL("../src-tauri/src/overlay.rs", import.meta.url), "utf8");
  const width = Number(overlay.match(/INDICATOR_LOGICAL_W: f64 = ([0-9.]+)/)[1]);
  const height = Number(overlay.match(/INDICATOR_LOGICAL_H: f64 = ([0-9.]+)/)[1]);
  assert.ok(width <= 260);
  assert.ok(height <= 64);
  assert.ok(width * height <= (480 * 132) / 4);
  const window = readFileSync(new URL("../src-tauri/src/dictation.rs", import.meta.url), "utf8");
  assert.match(window, new RegExp(`inner_size\\(${width}\\.0, ${height}\\.0\\)`));
  assert.match(window, /\.decorations\(false\)/);
  assert.match(window, /\.transparent\(true\)/);
  assert.match(window, /\.shadow\(false\)/);
});

test("status stays one language and the route chip stays short", () => {
  applyLanguage("zh", "zh");
  const zh = statusLine("口授", "空闲");
  assert.equal(zh, "口授 · 空闲");
  assert.equal(mixesLanguages(zh), false);
  assert.equal(routeBadge("本地", zhWords), "本地");
  assert.equal(routeBadge("本地自动", zhWords), "自动");
  assert.equal(routeBadge("严格离线", zhWords), "离线");
  assert.equal(routeBadge("云端", zhWords), "云端");
  for (const chip of ["本地", "自动", "离线", "云端"]) {
    assert.equal(mixesLanguages(chip), false);
  }

  applyLanguage("en", "en");
  const en = statusLine("Dictate", "Idle");
  assert.equal(en, "Dictate · Idle");
  assert.equal(mixesLanguages(en), false);
  assert.equal(routeBadge("Local", enWords), "Local");
  assert.equal(routeBadge("Local, automatic", enWords), "Auto");
  assert.equal(routeBadge("Strict offline", enWords), "Offline");
  assert.equal(routeBadge("Cloud", enWords), "Cloud");
  assert.equal(statusLine("Dictate", "0:04"), "Dictate · 0:04");
});
