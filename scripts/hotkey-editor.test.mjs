import assert from "node:assert/strict";
import test from "node:test";

import {
  beginCapture,
  captureHeardNothing,
  bindingError,
  bindingWarnings,
  cancelEditor,
  idleEditor,
  knownConflict,
  markApplied,
  showCaptured,
} from "../src/hotkeyEditor.ts";

test("capture with no keys after a few seconds is a miss", () => {
  assert.equal(captureHeardNothing("capturing", "", 2999), false);
  assert.equal(captureHeardNothing("capturing", "", 3000), true);
  assert.equal(captureHeardNothing("capturing", "Right Alt", 3000), false);
  assert.equal(captureHeardNothing("pending", "", 3000), false);
});

test("capture shows the chord and set applies it", () => {
  const idle = idleEditor("ControlRight", "Right Ctrl");
  const capturing = beginCapture(idle);
  assert.equal(capturing.phase, "capturing");
  assert.equal(capturing.activeChord, "ControlRight");
  assert.equal(capturing.pendingChord, "");

  const pending = showCaptured(capturing, "AltRight", "Right Alt");
  assert.equal(pending.phase, "pending");
  assert.equal(pending.pendingChord, "AltRight");
  assert.equal(pending.activeChord, "ControlRight");
  assert.match(knownConflict(pending.pendingChord), /Typeless/);

  const applied = markApplied(pending);
  assert.equal(applied.phase, "idle");
  assert.equal(applied.activeChord, "AltRight");
  assert.equal(applied.pendingChord, "");
});

test("cancel and recapture leave the saved chord in place until set", () => {
  const pending = showCaptured(beginCapture(idleEditor("ControlRight", "Right Ctrl")), "AltRight", "Right Alt");
  const cancelled = cancelEditor(pending);
  assert.equal(cancelled.phase, "idle");
  assert.equal(cancelled.activeChord, "ControlRight");
  assert.equal(cancelled.pendingChord, "");

  const again = beginCapture(pending);
  assert.equal(again.phase, "capturing");
  assert.equal(again.pendingChord, "");
  assert.equal(again.activeChord, "ControlRight");
  const escaped = cancelEditor(again);
  assert.equal(escaped.activeChord, "ControlRight");
});

test("saved alt chords stay distinct and still warn about typeless", () => {
  const saved = ["AltRight", "AltRight+ShiftRight", "AltRight+Enter"];
  assert.equal(bindingError(saved), null);
  const warnings = bindingWarnings(saved);
  assert.ok(warnings.some((warning) => warning.includes("Typeless")));
  assert.ok(warnings.some((warning) => warning.includes("short press")));
  assert.equal(bindingError(["AltRight", "AltRight", "AltRight+Enter"]), "Dictate and Translate use the same hotkey.");
  assert.equal(knownConflict("ControlRight"), null);
});
