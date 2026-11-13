import { routeBadge, routeKind, statusLine, taskbarStripVars, type RouteWords } from "./indicator-layout";
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
      <span class="grip" id="grip" title="${escapeHtml(t("drag"))}" aria-hidden="true"></span>
      <span class="dot" id="dot"></span>
      <strong id="phase">${escapeHtml(t("idle"))}</strong>
      <span id="route" class="badge"></span>
      <span class="tools">
        <span class="bar-actions" id="actions" hidden>
          <button type="button" class="icon" id="copy" title="${escapeHtml(t("copy"))}" aria-label="${escapeHtml(t("copy"))}">${iconCopy()}</button>
          <button type="button" class="icon" id="reinsert" title="${escapeHtml(t("insert"))}" aria-label="${escapeHtml(t("insert"))}">${iconInsert()}</button>
        </span>
        <button type="button" class="icon" id="history" title="${escapeHtml(t("history"))}" aria-label="${escapeHtml(t("history"))}">${iconHistory()}</button>
        <button type="button" class="icon" id="cancel" title="${escapeHtml(t("cancel"))}" aria-label="${escapeHtml(t("cancel"))}">${iconCancel()}</button>
      </span>
      <p id="message" class="sr"></p>
    </div>
  `;
  const bar = document.querySelector<HTMLDivElement>("#bar");
  bar?.addEventListener("pointerdown", (event) => {
    const target = event.target as HTMLElement | null;
    if (target?.closest("button")) {
      return;
    }
    if (isPreview() || document.body.dataset.place === "taskbar") {
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
  document.body.dataset.place = snap.settings.indicatorPlace || "floating";
  watchStripSize();
  relabel();
  await refreshLatest();
  await listen<PhaseEvent>("dictation://phase", (event) => {
    render(event.payload);
    if (event.payload.phase !== "recording") {
      void refreshLatest();
    }
  });
}

function watchStripSize(): void {
  const apply = () => applyStripScale();
  apply();
  new MutationObserver(apply).observe(document.body, {
    attributes: true,
    attributeFilter: ["data-place"],
  });
  new ResizeObserver(apply).observe(document.documentElement);
  window.addEventListener("resize", apply);
}

function applyStripScale(): void {
  const root = document.documentElement;
  if (document.body.dataset.place !== "taskbar") {
    root.style.removeProperty("--strip-font");
    root.style.removeProperty("--strip-icon");
    root.style.removeProperty("--strip-control");
    root.style.removeProperty("--strip-dot");
    root.style.removeProperty("--strip-grip");
    return;
  }
  const height = root.clientHeight || window.innerHeight;
  const vars = taskbarStripVars(height);
  root.style.setProperty("--strip-font", `${vars.fontPx}px`);
  root.style.setProperty("--strip-icon", `${vars.iconPx}px`);
  root.style.setProperty("--strip-control", `${vars.controlPx}px`);
  root.style.setProperty("--strip-dot", `${vars.dotPx}px`);
  root.style.setProperty("--strip-grip", `${vars.gripPx}px`);
}

function relabel(): void {
  const phase = document.querySelector("#phase");
  if (phase && !phase.textContent) {
    phase.textContent = t("idle");
  }
  setTip("#history", "history");
  setTip("#cancel", "cancel");
  setTip("#copy", "copy");
  setTip("#reinsert", "insert");
  document.querySelector("#grip")?.setAttribute("title", t("drag"));
}

function routeWords(): RouteWords {
  return { auto: ui("Auto", "自动"), offline: ui("Offline", "离线"), local: ui("Local", "本地"), cloud: ui("Cloud", "云端") };
}

function setTip(selector: string, key: "copy" | "insert" | "history" | "cancel"): void {
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
  if (!bar || !phase || !route || !message) {
    return;
  }
  bar.dataset.phase = event.phase;
  bar.dataset.path = routeKind(event.route);
  const title = statusLine(event.mode ?? "", label(event.phase, event.elapsedMs));
  phase.textContent = title;
  const detail = event.message ? `${title}. ${event.message}` : title;
  phase.setAttribute("title", detail);
  route.textContent = routeBadge(event.route, routeWords());
  route.setAttribute("title", event.route);
  message.textContent = event.message;
}

function label(phase: string, elapsedMs: number): string {
  const seconds = Math.floor(elapsedMs / 1000);
  const clock = `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
  switch (phase) {
    case "recording":
      return clock;
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

function iconCancel(): string {
  return `<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4.5 4.5 11.5 11.5M11.5 4.5 4.5 11.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>`;
}
