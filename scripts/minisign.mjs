import crypto from "node:crypto";

const ED25519_SPKI_PREFIX = Buffer.from("302a300506032b6570032100", "hex");

function untrustedComment(text, label) {
  const lines = text.replace(/^\uFEFF/, "").split(/\r?\n/);
  const comment = lines[0] ?? "";
  if (!comment.startsWith("untrusted comment: ")) {
    throw new Error(`${label} is missing an untrusted comment`);
  }
  const body = (lines[1] ?? "").trim();
  if (!body) {
    throw new Error(`${label} is missing its key data`);
  }
  return { lines, body };
}

function rawPublicKey(publicKeyText) {
  const { body } = untrustedComment(publicKeyText, "public key");
  const decoded = Buffer.from(body, "base64");
  if (decoded.length !== 42) {
    throw new Error("public key did not decode to a minisign key");
  }
  const algorithm = decoded.subarray(0, 2).toString("utf8");
  if (algorithm !== "Ed") {
    throw new Error(`unsupported minisign public key algorithm ${algorithm}`);
  }
  return {
    keyId: decoded.subarray(2, 10),
    key: crypto.createPublicKey({
      key: Buffer.concat([ED25519_SPKI_PREFIX, decoded.subarray(10)]),
      format: "der",
      type: "spki",
    }),
  };
}

function rawSignature(signatureText) {
  const { lines, body } = untrustedComment(signatureText, "signature");
  const decoded = Buffer.from(body, "base64");
  if (decoded.length !== 74) {
    throw new Error("signature did not decode to a minisign signature");
  }
  const trustedLine = lines[2] ?? "";
  if (!trustedLine.startsWith("trusted comment: ")) {
    throw new Error("signature is missing a trusted comment");
  }
  const trusted = trustedLine.slice("trusted comment: ".length).trim();
  if (!trusted) {
    throw new Error("trusted comment is empty");
  }
  const global = Buffer.from((lines[3] ?? "").trim(), "base64");
  if (global.length !== 64) {
    throw new Error("comment signature is not an Ed25519 signature");
  }
  return {
    algorithm: decoded.subarray(0, 2).toString("utf8"),
    keyId: decoded.subarray(2, 10),
    signature: decoded.subarray(10),
    trusted,
    global,
  };
}

function prehash(message) {
  return crypto.createHash("blake2b512").update(message).digest();
}

export function verifyMinisign({ publicKeyText, signatureText, message }) {
  const pub = rawPublicKey(publicKeyText);
  const sig = rawSignature(signatureText);
  if (!pub.keyId.equals(sig.keyId)) {
    throw new Error("signature key id does not match the public key");
  }
  const signed = sig.algorithm === "ED" ? prehash(message) : sig.algorithm === "Ed" ? message : null;
  if (!signed) {
    throw new Error(`unsupported minisign signature algorithm ${sig.algorithm}`);
  }
  if (!crypto.verify(null, signed, pub.key, sig.signature)) {
    throw new Error("minisign signature verification failed");
  }
  const commentMessage = Buffer.concat([sig.signature, Buffer.from(sig.trusted, "utf8")]);
  if (!crypto.verify(null, commentMessage, pub.key, sig.global)) {
    throw new Error("minisign trusted comment verification failed");
  }
}
