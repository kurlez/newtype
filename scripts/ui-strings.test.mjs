import assert from "node:assert/strict";
import test from "node:test";
import { mixesLanguages, uiTable } from "../src/i18n.ts";

test("every ui key has one english string and one chinese string", () => {
  const table = uiTable();
  const keys = Object.keys(table);
  assert.ok(keys.length > 0);
  for (const key of keys) {
    const pair = table[key];
    assert.equal(pair.length, 2, key);
    const [en, zh] = pair;
    assert.ok(en.length > 0 && zh.length > 0, key);
    assert.equal(mixesLanguages(en), false, en);
    assert.equal(mixesLanguages(zh), false, zh);
  }
});

test("a bilingual duplicate is rejected", () => {
  assert.equal(mixesLanguages("Local · manual · 本地 · 手动"), true);
  assert.equal(mixesLanguages("Cloud"), false);
  assert.equal(mixesLanguages("云端"), false);
  assert.equal(mixesLanguages("DashScope"), false);
});
