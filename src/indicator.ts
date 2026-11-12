import { previewPhases } from "./preview";
import { escapeHtml, isPreview, type PhaseEvent } from "./types";

const app = document.querySelector<HTMLDivElement>("#app");

export async function mount(): Promise<void> {
  if (!app) {
    return;
  }
  document.body.classList.add("indicator");
  app.innerHTML = `
    <div class="bar" id="bar">
      <div class="bar-top">
        <span class="dot" id="dot"></span>
        <strong id="phase">Idle</strong>
        <span id="route"></span>
        <button type="button" id="cancel">Cancel</button>
      </div>
      <div class="meter" id="meter" aria-hidden="true"></div>
      <p id="message"></p>
    </div>
  `;
  const cancel = document.querySelector<HTMLButtonElement>("#cancel");
  cancel?.addEventListener("click", () => {
    if (isPreview()) {
      render({ phase: "cancelled", route: "Cloud · 云端", message: "Cancelled. Nothing was inserted.", elapsedMs: 0, level: 0, text: "" });
      return;
    }
    void import("@tauri-apps/api/core").then(({ invoke }) => invoke("dictation_cancel"));
  });
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
    return;
  }
  const { listen } = await import("@tauri-apps/api/event");
  await listen<PhaseEvent>("dictation://phase", (event) => render(event.payload));
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
  message.textContent = event.text ? `${event.message} ${event.text}` : event.message;
  const bars = 8;
  const filled = Math.round(Math.min(1, Math.max(0, event.level)) * bars);
  meter.innerHTML = Array.from({ length: bars }, (_, index) => `<i class="${index < filled ? "on" : ""}"></i>`).join("");
  if (event.phase !== "recording" && event.phase !== "processing") {
    meter.innerHTML = "";
  }
  void escapeHtml;
}

function pathKind(route: string): string {
  const lower = route.toLowerCase();
  if (lower.includes("fallback") || lower.includes("回落")) {
    return "fallback";
  }
  if (lower.includes("offline") || lower.includes("离线")) {
    return "strict";
  }
  if (lower.includes("local") || lower.includes("本地")) {
    return "local";
  }
  return "cloud";
}

function label(phase: string, elapsedMs: number): string {
  const seconds = Math.floor(elapsedMs / 1000);
  const clock = `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
  switch (phase) {
    case "recording":
      return `Recording ${clock}`;
    case "processing":
      return "Processing";
    case "inserted":
      return "Inserted";
    case "fallback":
      return "Inserted";
    case "error":
      return "Error";
    case "silence":
      return "No speech";
    case "stub":
      return "Not in this build";
    case "cancelled":
      return "Cancelled";
    case "withheld":
      return "Not inserted";
    case "copied":
      return "Copied";
    default:
      return "Idle";
  }
}
