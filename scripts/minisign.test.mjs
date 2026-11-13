import assert from "node:assert/strict";
import crypto from "node:crypto";
import test from "node:test";
import { verifyMinisign } from "./minisign.mjs";

test("blake2b512 matches the empty-string vector", () => {
  const digest = crypto.createHash("blake2b512").update(Buffer.alloc(0)).digest("hex");
  assert.equal(
    digest,
    "786a02f742015903c6c6fd852552d272912f4740e15847618a86e217f71f5419d25e1031afee585313896444934eb04b903a685b1448b755d56f701afe9be2ce",
  );
});

function fixture(message, { tamper = false, comment = "timestamp:1\tfile:artifact\thashed" } = {}) {
  const { publicKey, privateKey } = crypto.generateKeyPairSync("ed25519");
  const spki = publicKey.export({ format: "der", type: "spki" });
  const rawPk = spki.subarray(spki.length - 32);
  const keyId = Buffer.from("0102030405060708", "hex");
  const hash = crypto.createHash("blake2b512").update(message).digest();
  const signature = crypto.sign(null, hash, privateKey);
  const global = crypto.sign(null, Buffer.concat([signature, Buffer.from(comment, "utf8")]), privateKey);
  const sigStruct = Buffer.concat([Buffer.from("ED"), keyId, signature]);
  const pubStruct = Buffer.concat([Buffer.from("Ed"), keyId, rawPk]);
  const signatureText = [
    "untrusted comment: signature from minisign secret key",
    sigStruct.toString("base64"),
    `trusted comment: ${comment}`,
    global.toString("base64"),
    "",
  ].join("\n");
  const publicKeyText = ["untrusted comment: minisign public key: TEST", pubStruct.toString("base64"), ""].join("\n");
  return {
    publicKeyText,
    signatureText,
    message: tamper ? Buffer.concat([message, Buffer.from("!")]) : message,
  };
}

test("a prehashed minisign signature verifies", () => {
  const sample = fixture(Buffer.from("updater-bytes"));
  assert.doesNotThrow(() => verifyMinisign(sample));
});

test("a changed artifact or comment is rejected", () => {
  const sample = fixture(Buffer.from("updater-bytes"), { tamper: true });
  assert.throws(() => verifyMinisign(sample), /signature verification failed/);
  const broken = fixture(Buffer.from("updater-bytes"));
  broken.signatureText = broken.signatureText.replace("timestamp:1", "timestamp:2");
  assert.throws(() => verifyMinisign(broken), /trusted comment/);
});
