import assert from "node:assert/strict";
import test from "node:test";
import { decideTarget, planPackagePrune } from "./runner/prune-cache.mjs";

const day = 24 * 60 * 60 * 1000;
const now = Date.UTC(2026, 10, 12);
const cap = 8 * 1024 * 1024 * 1024;

test("a fresh target under the cap is kept", () => {
  assert.equal(decideTarget({ bytes: 1000, newestMs: now - day, now, maxBytes: cap, maxAgeMs: 14 * day }), "keep");
});

test("a target that has not been used for two weeks is deleted", () => {
  assert.equal(
    decideTarget({ bytes: 1000, newestMs: now - 15 * day, now, maxBytes: cap, maxAgeMs: 14 * day }),
    "delete-stale",
  );
});

test("a target over the cap is deleted", () => {
  assert.equal(decideTarget({ bytes: cap + 1, newestMs: now, now, maxBytes: cap, maxAgeMs: 14 * day }), "delete-over-cap");
});

test("a missing target is left alone", () => {
  assert.equal(decideTarget({ bytes: 0, newestMs: 0, now, maxBytes: cap, maxAgeMs: 14 * day }), "keep");
});

test("a crate checkout without Cargo.toml is removed even when the directory is new", () => {
  assert.equal(
    planPackagePrune({ hasManifest: false, dirMtimeMs: now, cutoffMs: now - 14 * day, dropBroken: true }),
    "delete-broken",
  );
});

test("a fresh crate checkout is kept as a directory, not by the age of files inside it", () => {
  assert.equal(
    planPackagePrune({ hasManifest: true, dirMtimeMs: now - day, cutoffMs: now - 14 * day, dropBroken: true }),
    "keep",
  );
});
