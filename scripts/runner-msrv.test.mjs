import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function rustVersion(file, pattern) {
  const text = readFileSync(file, "utf8");
  const match = text.match(pattern);
  assert.ok(match, `${file} is missing the minimum rust version`);
  return match[1];
}

test("standalone runner scripts match Cargo.toml rust-version", () => {
  const cargo = rustVersion(
    path.join(root, "src-tauri/Cargo.toml"),
    /^rust-version = "([^"]+)"/m,
  );
  const mac = rustVersion(
    path.join(root, "scripts/runner/setup-mac.sh"),
    /^minimum_rust="([^"]+)"/m,
  );
  const windows = rustVersion(
    path.join(root, "scripts/runner/setup-windows.ps1"),
    /^\$MinimumRust = '([^']+)'/m,
  );
  assert.equal(mac, cargo);
  assert.equal(windows, cargo);
});

test("windows setup does not call svc.cmd and defaults to LocalSystem", () => {
  const text = readFileSync(path.join(root, "scripts/runner/setup-windows.ps1"), "utf8");
  assert.equal(text.includes("svc.cmd"), false);
  assert.equal(text.includes("NT AUTHORITY\\SYSTEM"), true);
  assert.equal(text.includes("CARGO_HOME="), true);
  assert.equal(text.includes("RUSTUP_HOME="), true);
});
