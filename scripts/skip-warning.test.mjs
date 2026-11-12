import assert from "node:assert/strict";
import test from "node:test";
import { confirmSkip } from "../src/skipWarning.ts";

test("skip is recorded only after the warning is accepted", () => {
  let asked = "";
  assert.equal(
    confirmSkip("断网后无法自动改用本地模型", (text) => {
      asked = text;
      return false;
    }),
    false,
  );
  assert.equal(asked, "断网后无法自动改用本地模型");
  assert.equal(confirmSkip("both speech and rewrite", () => true), true);
  assert.equal(confirmSkip("   ", () => true), false);
  assert.equal(confirmSkip("", () => true), false);
});
