import { applyLanguage, t, ui } from "./i18n";
import { previewPhases } from "./preview";
import { escapeHtml, isPreview, type HistoryEntry, type PhaseEvent, type Snapshot } from "./types";

const app = document.querySelector<HTMLDivElement>("#app");
let latestId = "";

export async function mount(): Promise<void> {
  if (!app) {
    return;
  }
  document.documentElement.classList.add("indicator");
  document.body.classList.add("indicator");
  app.innerHTML = `
    <div class="bar" id="bar">
      <div class="bar-top">
        <span class="grip" id="grip" title="${escapeHtml(t("drag"))}" aria-hidden="true"></span>
        <span class="dot" id="dot"></span>
        <strong id="phase">${escapeHtml(t("idle"))}</strong>
        <span id="route"></span>
        <button type="button" id="history" title="${escapeHtml(t("history"))}">${escapeHtml(t("history"))}</button>
        <button type="button" id="cancel" title="${escapeHtml(t("cancel"))}">${escapeHtml(t("cancel"))}</button>
      </div>
      <div class="bar-actions" id="actions" hidden>
        <button type="button" class="icon" id="copy" title="${escapeHtml(t("copy"))}" aria-label="${escapeHtml(t("copy"))}">${iconCopy()}</button>
        <button type="button" class="icon" id="reinsert" title="${escapeHtml(t("insert"))}" aria-label="${escapeHtml(t("insert"))}">${iconInsert()}</button>
        <button type="button" class="icon" id="open-history" title="${escapeHtml(t("openHistory"))}" aria-label="${escapeHtml(t("openHistory"))}">${iconHistory()}</button>
      </div>
      <div class="meter" id="meter" aria-hidden="true"></div>
      <p id="message"></p>
    </div>
  `;
  const bar = document.querySelector<HTMLDivElement>("#bar");
  bar?.addEventListener("pointerdown", (event) => {
    const target = event.target as HTMLElement | null;
    if (target?.closest("button")) {
      return;
    }
    if (isPreview()) {
      return;
    }
    void import("@tauri-apps/api/window").then(async ({ getCurrentWindow }) => {
      const win = getCurrentWindow();
      await win.startDragging();
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("indicator_save_position");
    });
  });
  document.querySelector("#cancel")?.addEventListener("click", () => {
    if (isPreview()) {
      render({ phase: "cancelled", route: ui("Cloud", "云端"), message: ui("Cancelled. Nothing was inserted.", "已取消。没有插入。"), elapsedMs: 0, level: 0, text: "" });
      return;
    }
    void import("@tauri-apps/api/core").then(({ invoke }) => invoke("dictation_cancel"));
  });
  document.querySelector("#history")?.addEventListener("click", () => void openHistory(false));
  document.querySelector("#copy")?.addEventListener("click", () => void act("dictation_copy_history"));
  document.querySelector("#reinsert")?.addEventListener("click", () => void act("dictation_reinsert_history"));
  document.querySelector("#open-history")?.addEventListener("click", () => void openHistory(true));
  if (isPreview()) {
    let index = 0;
    const next = document.createElement("button");
    next.type = "button";
    next.id = "preview-next";
    next.textContent = "Preview next state";
    next.addEventListener("click", () => {
      const phase = previewPhases[index % previewPhases.length];
      if (phase) {
        render(phase);
      }
      index += 1;
    });
    app.append(next);
    const first = previewPhases[0];
    if (first) {
      render(first);
    }
    showActions(true);
    return;
  }
  const { listen } = await import("@tauri-apps/api/event");
  const { invoke } = await import("@tauri-apps/api/core");
  const snap = await invoke<Snapshot>("dictation_snapshot");
  applyLanguage(snap.settings.uiLanguage || "auto", snap.language || "");
  relabel();
  await refreshLatest();
  await listen<PhaseEvent>("dictation://phase", (event) => {
    render(event.payload);
    if (event.payload.phase !== "recording") {
      void refreshLatest();
    }
  });
}

function relabel(): void {
  const phase = document.querySelector("#phase");
  if (phase && !phase.textContent) {
    phase.textContent = t("idle");
  }
  const history = document.querySelector("#history");
  if (history) {
    history.textContent = t("history");
    history.setAttribute("title", t("history"));
  }
  const cancel = document.querySelector("#cancel");
  if (cancel) {
    cancel.textContent = t("cancel");
    cancel.setAttribute("title", t("cancel"));
  }
  setTip("#copy", "copy");
  setTip("#reinsert", "insert");
  setTip("#open-history", "openHistory");
  document.querySelector("#grip")?.setAttribute("title", t("drag"));
}

function setTip(selector: string, key: "copy" | "insert" | "openHistory"): void {
  const node = document.querySelector(selector);
  node?.setAttribute("title", t(key));
  node?.setAttribute("aria-label", t(key));
}

async function refreshLatest(): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  const rows = await invoke<HistoryEntry[]>("dictation_history");
  latestId = rows[0]?.id ?? "";
  showActions(Boolean(latestId));
}

function showActions(on: boolean): void {
  document.querySelector("#actions")?.toggleAttribute("hidden", !on);
}

async function act(command: string): Promise<void> {
  if (!latestId || isPreview()) {
    return;
  }
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke(command, { id: latestId });
}

async function openHistory(last: boolean): Promise<void> {
  if (isPreview()) {
    return;
  }
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("indicator_open_history", { last });
}

function render(event: PhaseEvent): void {
  const bar = document.querySelector<HTMLDivElement>("#bar");
  const phase = document.querySelector("#phase");
  const route = document.querySelector("#route");
  const message = document.querySelector("#message");
  const meter = document.querySelector("#meter");
  if (!bar || !phase || !route || !message || !meter) {
    return;
  }
  bar.dataset.phase = event.phase;
  bar.dataset.path = pathKind(event.route);
  const title = label(event.phase, event.elapsedMs);
  phase.textContent = event.mode ? `${event.mode} · ${title}` : title;
  route.textContent = event.route;
  message.textContent = event.message;
  const bars = 8;
  const filled = Math.round(Math.min(1, Math.max(0, event.level)) * bars);
  meter.innerHTML = Array.from({ length: bars }, (_, index) => `<i class="${index < filled ? "on" : ""}"></i>`).join("");
  if (event.phase !== "recording" && event.phase !== "processing") {
    meter.innerHTML = "";
  }
}

function pathKind(route: string): string {
  if (/fallback|automatic|自动|回落/.test(route)) {
    return "fallback";
  }
  if (/offline|离线/.test(route)) {
    return "strict";
  }
  if (/local|本地/i.test(route)) {
    return "local";
  }
  return "cloud";
}

function label(phase: string, elapsedMs: number): string {
  const seconds = Math.floor(elapsedMs / 1000);
  const clock = `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
  switch (phase) {
    case "recording":
      return `${t("recording")} ${clock}`;
    case "processing":
      return t("processing");
    case "inserted":
    case "fallback":
      return t("inserted");
    case "error":
      return t("error");
    case "silence":
      return t("silence");
    case "stub":
      return t("stub");
    case "cancelled":
      return t("cancelled");
    case "withheld":
      return t("withheld");
    case "copied":
      return t("copied");
    default:
      return t("idle");
  }
}

function iconCopy(): string {
  return `<svg viewBox="0 0 16 16" aria-hidden="true"><rect x="5" y="5" width="8" height="8" rx="1" fill="none" stroke="currentColor" stroke-width="1.4"/><path d="M4 11H3.2A1.2 1.2 0 0 1 2 9.8V3.2A1.2 1.2 0 0 1 3.2 2H9.8A1.2 1.2 0 0 1 11 3.2V4" fill="none" stroke="currentColor" stroke-width="1.4"/></svg>`;
}

function iconInsert(): string {
  return `<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 8h7M8 4.5 11.5 8 8 11.5" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/><path d="M13 3v10" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>`;
}

function iconHistory(): string {
  return `<svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="5.2" fill="none" stroke="currentColor" stroke-width="1.4"/><path d="M8 5.2V8l2 1.4" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>`;
}
