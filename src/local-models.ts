import { confirmSkip } from "./skipWarning";
import { escapeHtml, type LocalLibrary, type LocalProgress, type Snapshot } from "./types";

let wired = false;
let pendingDelete = "";

export function localPanel(local: LocalLibrary, preference: string): string {
  const reminder = local.reminder
    ? `<p class="status warn">${escapeHtml(local.reminder)}</p>`
    : `<p class="status">Offline fallback is ready. Both local models are installed.</p>`;
  const rows = local.models
    .map((model) => {
      const mark = model.recommended ? "Recommended. " : "";
      const active = model.active ? "In use for the next local dictation. " : "";
      const size = formatBytes(model.sizeBytes);
      const got = model.state === "partial" || model.state === "downloading" ? ` ${formatBytes(model.receivedBytes)} received.` : "";
      const action = model.state === "installed"
        ? `<button type="button" data-local-use="${escapeHtml(model.role)}:${escapeHtml(model.id)}">${model.active ? "In use" : "Use"}</button><button type="button" data-local-delete="${escapeHtml(model.id)}">${pendingDelete === model.id ? "Delete files" : "Delete"}</button>`
        : `<button type="button" data-local-download="${escapeHtml(model.id)}" ${model.available ? "" : "disabled"}>${model.state === "partial" ? "Resume" : "Download"}</button>`;
      return `<article class="model">
        <h3>${escapeHtml(model.name)} <span class="badge ${model.state === "installed" ? "" : "warn"}">${escapeHtml(model.state)}</span></h3>
        <p class="status">${escapeHtml(mark + active + model.notes)}</p>
        <p class="status">${escapeHtml(model.license)} · ${escapeHtml(model.runtime)} · ${escapeHtml(size)} · ${escapeHtml(model.memory)}</p>
        <p class="status">${escapeHtml(model.languages)} ${escapeHtml(model.message)}${escapeHtml(got)}</p>
        <div class="row">${action}</div>
      </article>`;
    })
    .join("");
  const strict = preference === "strictOffline"
    ? `<p class="status warn">Strict offline is on. Downloads, probes, and update checks stay off until you choose another path.</p>`
    : "";
  return `<section id="local-models" data-asr="${escapeHtml(local.recommendedAsr)}" data-llm="${escapeHtml(local.recommendedLlm)}" data-skip-warning="${escapeHtml(local.skipWarning)}">
    <h2>Local models</h2>
    ${reminder}
    <p class="status">${escapeHtml(local.hardwareSummary)}</p>
    <p class="status">Disk used by local models: ${escapeHtml(formatBytes(local.diskBytes))}. Opening this page does not load the weights.</p>
    ${strict}
    <label class="field">Download source
      <select id="local-mirror">
        ${["auto", "huggingface", "hf-mirror", "modelscope", "github"].map((value) => `<option value="${value}" ${local.mirror === value ? "selected" : ""}>${escapeHtml(mirrorLabel(value))}</option>`).join("")}
      </select>
    </label>
    <div class="row">
      <button type="button" id="local-recommended" class="primary">Download recommended</button>
      ${local.skipped ? "" : `<button type="button" id="local-skip">Skip for now</button>`}
      <button type="button" id="local-cancel" ${local.downloading ? "" : "hidden"}>Cancel download</button>
    </div>
    <p class="status" id="local-progress"></p>
    <progress id="local-bar" hidden max="1" value="0"></progress>
    ${rows}
  </section>`;
}

function confirmNetwork(): boolean | null {
  const strict = document.querySelector<HTMLInputElement>('input[name="preference"][value="strictOffline"]')?.checked === true;
  if (!strict) {
    return false;
  }
  const line = document.querySelector("#local-progress");
  if (line) {
    line.textContent = "Strict offline is on. Choose Auto, Cloud only, or Local only before downloading. Nothing was contacted.";
  }
  return null;
}

export function bindLocal(preview: boolean, reloadLocal: () => Promise<void>): void {
  document.querySelector("#local-mirror")?.addEventListener("change", (event) => {
    const mirror = (event.target as HTMLSelectElement).value;
    void run(preview, reloadLocal, () => call("local_set_mirror", { mirror }));
  });
  document.querySelector("#local-recommended")?.addEventListener("click", () => {
    const host = document.querySelector("#local-models");
    const asr = host?.getAttribute("data-asr") ?? "";
    const llm = host?.getAttribute("data-llm") ?? "";
    const allow = confirmNetwork();
    if (allow === null) {
      return;
    }
    void startDownload(preview, reloadLocal, [asr, llm].filter(Boolean), allow);
  });
  document.querySelector("#local-skip")?.addEventListener("click", () => {
    const message = document.querySelector("#local-models")?.getAttribute("data-skip-warning") ?? "";
    if (!confirmSkip(message, (text) => window.confirm(text))) {
      return;
    }
    void run(preview, reloadLocal, () => call("local_skip_download"));
  });
  document.querySelector("#local-cancel")?.addEventListener("click", () => {
    if (preview) {
      return;
    }
    void call("local_cancel_download");
  });
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-local-download]")) {
    button.addEventListener("click", () => {
      const id = button.dataset.localDownload ?? "";
      const allow = confirmNetwork();
      if (allow === null) {
        return;
      }
      void startDownload(preview, reloadLocal, [id], allow);
    });
  }
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-local-delete]")) {
    button.addEventListener("click", () => {
      const id = button.dataset.localDelete ?? "";
      if (pendingDelete !== id) {
        pendingDelete = id;
        void reloadLocal();
        return;
      }
      pendingDelete = "";
      void run(preview, reloadLocal, () => call("local_delete", { id }));
    });
  }
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-local-use]")) {
    button.addEventListener("click", () => {
      const [role, id] = (button.dataset.localUse ?? "").split(":");
      if (!role || !id) {
        return;
      }
      void run(preview, reloadLocal, () => call("local_use_model", { role, id }));
    });
  }
}

export function wireLocal(preview: boolean, onProgress: (event: LocalProgress) => void): void {
  if (wired || preview) {
    return;
  }
  wired = true;
  void import("@tauri-apps/api/event").then(({ listen }) => {
    void listen<LocalProgress>("local://progress", (event) => onProgress(event.payload));
  });
}

export function showProgress(event: LocalProgress): void {
  const line = document.querySelector("#local-progress");
  const bar = document.querySelector<HTMLProgressElement>("#local-bar");
  if (line) {
    line.textContent = event.message;
  }
  if (!bar) {
    return;
  }
  if (event.phase === "downloading" && event.total > 0) {
    bar.hidden = false;
    bar.max = event.total;
    bar.value = event.received;
  } else {
    bar.hidden = true;
  }
}

function startDownload(preview: boolean, reloadLocal: () => Promise<void>, ids: string[], confirmNetwork: boolean): void {
  if (preview) {
    const line = document.querySelector("#local-progress");
    if (line) {
      line.textContent = "Preview does not download models.";
    }
    return;
  }
  void call("local_download", { ids, confirmNetwork }).then(() => reloadLocal()).catch(async (error: unknown) => {
    const line = document.querySelector("#local-progress");
    if (line) {
      line.textContent = String(error);
    }
    await reloadLocal();
  });
}

async function run(preview: boolean, reloadLocal: () => Promise<void>, action: () => Promise<Snapshot>): Promise<void> {
  if (preview) {
    const line = document.querySelector("#local-progress");
    if (line) {
      line.textContent = "Preview only. Local models are not changed.";
    }
    return;
  }
  try {
    const next = await action();
    remember(next);
    await reloadLocal();
  } catch (error) {
    const line = document.querySelector("#local-progress");
    if (line) {
      line.textContent = String(error);
    }
  }
}

function remember(next: Snapshot): void {
  const host = document.querySelector("#local-models");
  host?.setAttribute("data-snap", JSON.stringify({
    local: next.local,
    routeLabel: next.routeLabel,
    routeDetail: next.routeDetail,
    localMirror: next.settings.localMirror,
    localDownloadSkipped: next.settings.localDownloadSkipped,
    localAsrId: next.settings.localAsrId,
    localLlmId: next.settings.localLlmId,
  }));
}

export function takeRemembered(): {
  local: LocalLibrary;
  routeLabel: string;
  routeDetail: string;
  localMirror: string;
  localDownloadSkipped: boolean;
  localAsrId: string;
  localLlmId: string;
} | null {
  const host = document.querySelector("#local-models");
  const raw = host?.getAttribute("data-snap");
  if (!raw) {
    return null;
  }
  host?.removeAttribute("data-snap");
  return JSON.parse(raw) as ReturnType<typeof takeRemembered>;
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

function formatBytes(bytes: number): string {
  if (!bytes) {
    return "0 B";
  }
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 10 || unit === 0 ? 0 : 1)} ${units[unit]}`;
}

function mirrorLabel(value: string): string {
  switch (value) {
    case "huggingface":
      return "Hugging Face";
    case "hf-mirror":
      return "hf-mirror";
    case "modelscope":
      return "ModelScope";
    case "github":
      return "GitHub";
    default:
      return "Auto (Hugging Face, hf-mirror, ModelScope, GitHub)";
  }
}
