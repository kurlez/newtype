import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { collectRelease, mirrorDevPlan } from "./latest-json.mjs";
import { verifyMinisign } from "./minisign.mjs";
import { compareVersions } from "./with-version.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function arg(name, fallback) {
  const index = process.argv.indexOf(name);
  if (index === -1) {
    return fallback;
  }
  return process.argv[index + 1];
}

export function decodeTauriKey(encoded) {
  const text = Buffer.from(encoded.trim(), "base64").toString("utf8");
  if (!text.includes("untrusted comment:")) {
    throw new Error("updater key did not decode to a minisign key");
  }
  return text;
}

export function verifyUpdaterArtifact({ pubkey, artifact, signature }) {
  verifyMinisign({
    publicKeyText: decodeTauriKey(pubkey),
    signatureText: decodeTauriKey(signature),
    message: readFileSync(artifact),
  });
}

function gh(args, { allowFailure = false } = {}) {
  const result = spawnSync("gh", args, {
    encoding: "utf8",
    env: process.env,
  });
  if (result.status !== 0 && !allowFailure) {
    process.stderr.write(result.stderr || "");
    process.stderr.write(result.stdout || "");
    throw new Error(`gh ${args.join(" ")} failed`);
  }
  return result;
}

function ensureRepo(repo) {
  const head = gh(["api", `repos/${repo}/commits/HEAD`], { allowFailure: true });
  if (head.status === 0) {
    return;
  }
  const detail = `${head.stderr ?? ""}\n${head.stdout ?? ""}`;
  if (!/empty/i.test(detail)) {
    process.stderr.write(detail);
    throw new Error(
      `Cannot read ${repo}. NEWTYPE_RELEASES_TOKEN needs Contents: Read and write on that repository only.`,
    );
  }
  {
    const readme = [
      "# newtype releases",
      "",
      "Public update feeds for the private newtype app.",
      "",
      "- dev (every push to main): https://github.com/kurlez/newtype-releases/releases/download/dev/latest.json",
      "- stable (`v*` tags): https://github.com/kurlez/newtype-releases/releases/latest/download/latest.json",
      "",
      "Do not commit updater private keys.",
      "",
    ].join("\n");
    gh([
      "api",
      "--method",
      "PUT",
      `repos/${repo}/contents/README.md`,
      "-f",
      "message=Initialize newtype release repository",
      "-f",
      `content=${Buffer.from(readme).toString("base64")}`,
    ]);
  }
}

export function shouldMirrorToDev(stableVersion, devVersion) {
  if (!devVersion) {
    return true;
  }
  return compareVersions(devVersion, stableVersion) < 0;
}

function readDevVersion(repo, tag) {
  const dir = mkdtempSync(path.join(tmpdir(), "newtype-dev-feed-"));
  const downloaded = gh(
    ["release", "download", tag, "--repo", repo, "--pattern", "latest.json", "--dir", dir, "--clobber"],
    { allowFailure: true },
  );
  if (downloaded.status !== 0) {
    return null;
  }
  try {
    const body = JSON.parse(readFileSync(path.join(dir, "latest.json"), "utf8"));
    return typeof body.version === "string" ? body.version : null;
  } catch {
    return null;
  }
}

function publishPlan(repo, plan) {
  const existing = gh(["release", "view", plan.tag, "--repo", repo], { allowFailure: true });
  if (existing.status !== 0) {
    const args = [
      "release",
      "create",
      plan.tag,
      "--repo",
      repo,
      "--title",
      plan.prerelease ? "dev" : plan.tag,
      "--notes",
      plan.notes,
    ];
    if (plan.prerelease) {
      args.push("--prerelease");
    }
    gh(args);
  } else {
    const assetList = JSON.parse(
      gh(["release", "view", plan.tag, "--repo", repo, "--json", "assets"]).stdout,
    );
    for (const asset of assetList.assets) {
      gh(["release", "delete-asset", plan.tag, asset.name, "--repo", repo, "--yes"]);
    }
    const edit = ["release", "edit", plan.tag, "--repo", repo, "--notes", plan.notes];
    if (plan.prerelease) {
      edit.push("--prerelease");
    }
    gh(edit);
  }
  const latestDir = mkdtempSync(path.join(tmpdir(), "newtype-latest-"));
  const latestPath = path.join(latestDir, "latest.json");
  writeFileSync(latestPath, `${JSON.stringify(plan.latest, null, 2)}\n`);
  gh([
    "release",
    "upload",
    plan.tag,
    ...plan.uploadFiles,
    latestPath,
    "--repo",
    repo,
    "--clobber",
  ]);
  console.log(`published ${plan.version}+${plan.commit} to ${repo} tag ${plan.tag}`);
}

function main() {
  const artifacts = arg("--artifacts", "artifacts");
  const endpoints = JSON.parse(
    readFileSync(path.join(root, "src-tauri/update-endpoints.json"), "utf8"),
  );
  const repo = arg("--repo", endpoints.repo);
  const conf = JSON.parse(readFileSync(path.join(root, "src-tauri/tauri.conf.json"), "utf8"));
  const pubkey = conf.plugins.updater.pubkey;
  const placeholder = readFileSync(
    path.join(root, "src-tauri/placeholder-updater-pubkey.txt"),
    "utf8",
  ).trim();
  if (pubkey.trim() === placeholder) {
    console.error("plugins.updater.pubkey is still the committed placeholder.");
    console.error("Generate a keypair with `npm run tauri signer generate`, put the public key in tauri.conf.json, and store the private key as TAURI_SIGNING_PRIVATE_KEY.");
    console.error("See SETUP.md. Publishing was not changed.");
    process.exit(1);
  }
  const plan = collectRelease(artifacts, { repo, tag: endpoints.devTag });
  for (const platform of plan.platforms) {
    verifyUpdaterArtifact({
      pubkey,
      artifact: platform.file,
      signature: platform.signature,
    });
    console.log(`verified ${platform.platform} ${path.basename(platform.file)}`);
  }
  ensureRepo(repo);
  publishPlan(repo, plan);
  if (plan.channel === "stable" && shouldMirrorToDev(plan.version, readDevVersion(repo, endpoints.devTag))) {
    publishPlan(repo, mirrorDevPlan(plan, { repo, devTag: endpoints.devTag }));
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
