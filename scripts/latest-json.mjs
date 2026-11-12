import { readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";

export function classifyReleaseFile(filename) {
  if (filename.endsWith(".app.tar.gz.sig")) {
    return { role: "signature", artifact: filename.slice(0, -4) };
  }
  if (filename.endsWith("-setup.exe.sig")) {
    return { role: "signature", artifact: filename.slice(0, -4) };
  }
  if (filename.endsWith(".dmg")) {
    return { role: "installer" };
  }
  if (filename.endsWith(".app.tar.gz") || filename.endsWith("-setup.exe")) {
    return { role: "updater" };
  }
  return { role: "ignore" };
}

export function buildLatestJson({ version, notes, pubDate, platforms }) {
  if (!version) {
    throw new Error("latest.json needs a version");
  }
  const body = {
    version,
    notes: notes ?? "",
    pub_date: pubDate,
    platforms: {},
  };
  for (const entry of platforms) {
    if (!entry.platform || !entry.url || !entry.signature) {
      throw new Error(`incomplete platform entry: ${JSON.stringify(entry)}`);
    }
    body.platforms[entry.platform] = {
      signature: entry.signature.trim(),
      url: entry.url,
    };
  }
  return body;
}

export function collectRelease(artifactRoot, { repo, tag }) {
  const groups = new Map();
  for (const dirent of walk(artifactRoot)) {
    if (dirent.name !== "build-meta.json") {
      continue;
    }
    const dir = dirent.dir;
    const meta = JSON.parse(readFileSync(dirent.path, "utf8"));
    const files = readdirSync(dir);
    const signatures = new Map();
    const updaters = [];
    const installers = [];
    for (const name of files) {
      const kind = classifyReleaseFile(name);
      if (kind.role === "signature") {
        signatures.set(kind.artifact, readFileSync(path.join(dir, name), "utf8").trim());
      } else if (kind.role === "updater") {
        updaters.push(name);
      } else if (kind.role === "installer") {
        installers.push(name);
      }
    }
    if (updaters.length !== 1) {
      throw new Error(`${dir} should contain exactly one updater artifact, found ${updaters.join(", ") || "none"}`);
    }
    const artifact = updaters[0];
    const signature = signatures.get(artifact);
    if (!signature) {
      throw new Error(`${dir} is missing ${artifact}.sig`);
    }
    if (meta.updaterPlatform !== "darwin-aarch64" && meta.updaterPlatform !== "windows-x86_64") {
      throw new Error(`${dir} has no shipping updater platform (${meta.updaterPlatform})`);
    }
    groups.set(meta.updaterPlatform, {
      meta,
      artifact,
      signature,
      installers,
      dir,
    });
  }
  if (groups.size === 0) {
    throw new Error(`no build-meta.json under ${artifactRoot}`);
  }
  const metas = [...groups.values()].map((group) => group.meta);
  const version = metas[0].version;
  const commit = metas[0].commit;
  const channel = metas[0].channel;
  for (const meta of metas) {
    if (meta.version !== version || meta.commit !== commit || meta.channel !== channel) {
      throw new Error(`build metadata disagrees: ${JSON.stringify(metas)}`);
    }
  }
  const releaseTag = channel === "stable" ? `v${version}` : tag;
  const platforms = [...groups.entries()].map(([platform, group]) => ({
    platform,
    signature: group.signature,
    url: assetUrl(repo, releaseTag, group.artifact),
    file: path.join(group.dir, group.artifact),
  }));
  const uploadFiles = [];
  for (const group of groups.values()) {
    uploadFiles.push(path.join(group.dir, group.artifact));
    uploadFiles.push(path.join(group.dir, `${group.artifact}.sig`));
    for (const installer of group.installers) {
      uploadFiles.push(path.join(group.dir, installer));
    }
  }
  const notes = [
    `newtype ${version}+${commit}`,
    `channel: ${channel}`,
    `commit: ${commit}`,
    "",
    "Installers: the macOS .dmg and the Windows NSIS setup are attached for a first install.",
    "Later builds arrive through the in-app updater.",
  ].join("\n");
  return {
    version,
    commit,
    channel,
    tag: releaseTag,
    prerelease: channel !== "stable",
    notes,
    platforms,
    uploadFiles,
    latest: buildLatestJson({
      version,
      notes,
      pubDate: new Date().toISOString(),
      platforms,
    }),
  };
}

export function assetUrl(repo, tag, filename) {
  return `https://github.com/${repo}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(filename)}`;
}

function walk(directory) {
  const found = [];
  for (const name of readdirSync(directory)) {
    const full = path.join(directory, name);
    if (statSync(full).isDirectory()) {
      found.push(...walk(full));
    } else {
      found.push({ name, path: full, dir: directory });
    }
  }
  return found;
}
