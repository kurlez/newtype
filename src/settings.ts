import { applyLanguage, ui } from "./i18n";
import {
  beginCapture,
  CAPTURE_SILENCE_MS,
  captureHeardNothing,
  bindingError,
  bindingWarnings,
  cancelEditor,
  idleEditor,
  knownConflict,
  type HotkeyEditor,
  type HotkeyModeName,
  showCaptured,
} from "./hotkeyEditor";
import { bindLocal, localPanel, showProgress, takeRemembered, wireLocal } from "./local-models";
import { previewHistory, previewSnapshot } from "./preview";
import {
  escapeHtml,
  type CaptureEvent,
  type DictationSettings,
  type BundleInfo,
  type HistoryEntry,
  type Hotword,
  type PresetInfo,
  type ProviderConfig,
  type Snapshot,
  type UpdateState,
} from "./types";

const HOTKEY_MODES: Array<[HotkeyModeName, string, string]> = [
  ["dictate", "Dictate", "口授"],
  ["translate", "Translate", "翻译"],
  ["command", "Command", "指令"],
];

const TARGET_LANGUAGES = ["English", "简体中文", "繁体中文", "日本語", "Español", "Français", "Deutsch", "한국어", "Português"];

const PREVIEW_CHORDS: Array<[string, string]> = [
  ["AltRight", "Right Alt"],
  ["AltRight+ShiftRight", "Right Alt + Right Shift"],
  ["AltRight+Enter", "Right Alt + Enter"],
  ["ControlRight", "Right Ctrl"],
];

const editors: Record<HotkeyModeName, HotkeyEditor> = {
  dictate: idleEditor("", ""),
  translate: idleEditor("", ""),
  command: idleEditor("", ""),
};

let hotkeyMessage = "";
let hotkeyMessageError = false;
let captureTicket = 0;

const DICTATE_KEYS: Array<[string, string, string]> = [
  ["AltRight", "Right Alt", "右 Alt"],
  ["ControlRight", "Right Ctrl", "右 Ctrl"],
  ["ShiftRight", "Right Shift", "右 Shift"],
  ["CapsLock", "Caps Lock", "大写锁定"],
  ...Array.from({ length: 12 }, (_, index): [string, string, string] => {
    const name = `F${index + 1}`;
    return [name, name, name];
  }),
];

const root = document.querySelector<HTMLDivElement>("#app");
let snap: Snapshot = previewSnapshot;
let history: HistoryEntry[] = [];
let notice = "";
let preview = false;
let remoteModels: string[] = [];
let dictionaryQuery = "";
let historyQuery = "";
let mainTab: "history" | "settings" = "history";

export async function mount(previewMode: boolean): Promise<void> {
  preview = previewMode;
  if (!root) {
    return;
  }
  root.innerHTML = `
    <header>
      <h1>newtype</h1>
      <p id="tagline"></p>
    </header>
    <div id="dictate"></div>
    <section aria-labelledby="updates-heading">
      <h2 id="updates-heading"></h2>
      <p class="version" id="version-line">Loading…</p>
      <p class="endpoint" id="endpoint-line"></p>
      <fieldset>
        <legend id="channel-legend"></legend>
        <label class="choice"><input type="radio" name="channel" value="dev" /><span>dev<small id="channel-dev"></small></span></label>
        <label class="choice"><input type="radio" name="channel" value="stable" /><span>stable<small id="channel-stable"></small></span></label>
      </fieldset>
      <div class="row">
        <button type="button" id="check" class="primary"></button>
        <button type="button" id="apply" hidden></button>
      </div>
      <p class="status" id="status" aria-live="polite"></p>
      <progress id="progress" hidden max="1" value="0"></progress>
    </section>
    <section aria-labelledby="install-heading">
      <h2 id="install-heading"></h2>
      <ol id="install-steps"></ol>
      <p class="status" id="install-note"></p>
    </section>
    <section>
      <h2 id="about-heading"></h2>
      <p class="status" id="about-licenses">Cleanup prompts keep the MIT attribution for ckckck/OpenTypeless. No prompt text is taken from Open-Less/openless. Qwen3-ASR and Qwen3 weights are Apache-2.0. sherpa-onnx is Apache-2.0. llama.cpp is MIT. Silero VAD is MIT.</p>
    </section>
  `;
  wireUpdates();
  refreshChrome();
  if (preview) {
    snap = JSON.parse(JSON.stringify(previewSnapshot)) as Snapshot;
    history = previewHistory;
    paint();
    renderUpdate({
      version: "0.1.0-dev.0",
      commit: "preview",
      fullVersion: "0.1.0-dev.0+preview",
      channel: "dev",
      endpoint: "https://github.com/kurlez/newtype-releases/releases/download/dev/latest.json",
      phase: "idle",
      message: "Preview. Update checks run in the app.",
      availableVersion: null,
      receivedBytes: null,
      totalBytes: null,
    });
    return;
  }
  window.addEventListener("focus", () => {
    if (HOTKEY_MODES.some(([mode]) => editors[mode].phase === "capturing")) {
      return;
    }
    void reload();
  });
  const { listen } = await import("@tauri-apps/api/event");
  await listen<UpdateState>("update://state", (event) => renderUpdate(event.payload));
  await listen<{ routeLabel: string; routeDetail: string; routeNotice: string; connectivity: string }>("dictation://route", (event) => {
    snap.routeLabel = event.payload.routeLabel;
    snap.routeDetail = event.payload.routeDetail;
    snap.routeNotice = event.payload.routeNotice;
    snap.connectivity = event.payload.connectivity;
    const banner = document.querySelector("#route-banner");
    if (banner) {
      banner.textContent = snap.routeLabel;
    }
    const detail = document.querySelector("#route-detail");
    if (detail) {
      detail.textContent = snap.routeDetail;
    }
    const connectivity = document.querySelector("#connectivity");
    if (connectivity) {
      connectivity.textContent = snap.connectivity;
    }
    const routeNotice = document.querySelector<HTMLElement>("#route-notice");
    if (routeNotice) {
      routeNotice.textContent = snap.routeNotice;
      routeNotice.hidden = snap.routeNotice.length === 0;
    }
  });
  window.addEventListener("keydown", onCaptureKey, true);
  window.addEventListener("keyup", onCaptureKey, true);
  await listen<{ tab: string; last: boolean }>("ui://navigate", (event) => {
    if (event.payload.tab === "history" || event.payload.tab === "settings") {
      mainTab = event.payload.tab;
      paint();
      if (event.payload.last) {
        document.querySelector("#last-result")?.scrollIntoView({ block: "nearest" });
      }
    }
  });
  await listen<CaptureEvent>("dictation://capture", (event) => {
    const mode = event.payload.mode as HotkeyModeName;
    if (!editors[mode]) {
      return;
    }
    if (event.payload.phase === "listening") {
      editors[mode] = {
        ...editors[mode],
        phase: "capturing",
        pendingChord: event.payload.hotkey,
        pendingLabel: event.payload.label,
      };
      setHotkeyMessage(
        event.payload.label
          ? `${ui("Heard", "已按下")} ${event.payload.label}. ${ui("Release the keys to finish.", "松开按键后完成。")}`
          : ui("Press the keys, then release them. Escape cancels.", "按下按键后松开。Esc 取消。"),
      );
      renderHotkeys();
      return;
    }
    if (event.payload.phase === "idle" || !event.payload.hotkey) {
      editors[mode] = cancelEditor(editors[mode]);
      setHotkeyMessage("Capture cancelled. The saved hotkey is unchanged.");
    } else {
      editors[mode] = showCaptured(editors[mode], event.payload.hotkey, event.payload.label);
      const warning = knownConflict(event.payload.hotkey);
      setHotkeyMessage(
        warning
          ? `${event.payload.label} captured. ${warning} Click Set to keep it.`
          : `${event.payload.label} captured. Click Set to keep it.`,
      );
    }
    renderHotkeys();
  });
  const { invoke } = await import("@tauri-apps/api/core");
  renderUpdate(await invoke<UpdateState>("get_state"));
  wireLocal(preview, (event) => {
    showProgress(event);
    if (event.phase === "installed" || event.phase === "failed" || event.phase === "cancelled") {
      void refreshLocal();
    }
  });
  await reload();
}

async function refreshLocal(): Promise<void> {
  const remembered = takeRemembered();
  if (remembered) {
    snap.local = remembered.local;
    snap.routeLabel = remembered.routeLabel;
    snap.routeDetail = remembered.routeDetail;
    snap.settings.localMirror = remembered.localMirror;
    snap.settings.localDownloadSkipped = remembered.localDownloadSkipped;
    snap.settings.localAsrId = remembered.localAsrId;
    snap.settings.localLlmId = remembered.localLlmId;
  } else if (!preview) {
    const fresh = await call<Snapshot>("dictation_snapshot");
    snap.local = fresh.local;
    snap.routeLabel = fresh.routeLabel;
    snap.routeDetail = fresh.routeDetail;
    snap.settings.localMirror = fresh.settings.localMirror;
    snap.settings.localDownloadSkipped = fresh.settings.localDownloadSkipped;
    snap.settings.localAsrId = fresh.settings.localAsrId;
    snap.settings.localLlmId = fresh.settings.localLlmId;
  }
  const current = document.querySelector("#local-models");
  if (current) {
    current.outerHTML = localPanel(snap.local, snap.settings.preference);
    bindLocal(preview, refreshLocal);
  }
  const banner = document.querySelector("#route-banner");
  if (banner) {
    banner.textContent = snap.routeLabel;
  }
  const detail = document.querySelector("#route-detail");
  if (detail) {
    detail.textContent = snap.routeDetail;
  }
}

function windowSection(settings: DictationSettings): string {
  if (snap.permissions.platform !== "windows") {
    return "";
  }
  const checked = settings.windowTranslucent !== false ? "checked" : "";
  const place = settings.indicatorPlace || "taskbar";
  const option = (value: string, label: string) =>
    `<option value="${value}" ${place === value ? "selected" : ""}>${label}</option>`;
  return `<section id="sec-window">
      <h2>${ui("Window", "窗口")}</h2>
      <label class="choice"><input type="checkbox" id="window-translucent" ${checked} /><span>${ui("Translucent window", "半透明窗口")}<small>${ui("Windows 11 uses Mica or Acrylic. Windows 10 uses a light transparency. Text stays on a readable panel.", "Windows 11 使用 Mica 或 Acrylic。Windows 10 使用轻微透明。文字仍在可读的面板上。")}</small></span></label>
      <label>${ui("Indicator", "状态条")}
        <select id="indicator-place">
          ${option("taskbar", ui("Taskbar", "任务栏"))}
          ${option("floating", ui("Floating", "浮窗"))}
          ${option("hidden", ui("Hidden", "隐藏"))}
        </select>
        <small>${ui("Shown in the empty space just left of the clock. If that space is too small, the panel floats.", "显示在时钟左边的空位。空位不够时改用浮窗。")}</small>
      </label>
    </section>`;
}

function syncTranslucency(): void {
  const on = snap.settings.windowTranslucent !== false && snap.permissions.platform === "windows";
  document.documentElement.classList.toggle("translucent", on);
}

async function reload(): Promise<void> {
  snap = await call<Snapshot>("dictation_snapshot");
  history = await call<HistoryEntry[]>("dictation_history");
  paint();
}

function refreshChrome(): void {
  const set = (id: string, text: string) => {
    const node = document.querySelector(id);
    if (node) {
      node.textContent = text;
    }
  };
  set("#tagline", ui(
    "Cloud when the provider is reachable. Auto uses local models if it is not, and strict offline never leaves this computer.",
    "供应商能连通时走云端。不能连通时自动改用本地模型。严格离线不会离开这台电脑。",
  ));
  set("#updates-heading", ui("Updates", "更新"));
  set("#channel-legend", ui("Channel", "通道"));
  set("#channel-dev", ui("Every push to main.", "main 上的每次推送。"));
  set("#channel-stable", ui("Only version tags.", "只有版本标签。"));
  set("#check", ui("Check for updates", "检查更新"));
  set("#install-heading", ui("First install", "首次安装"));
  set("#about-heading", ui("About", "关于"));
  set("#install-note", ui(
    "Updates are not Developer ID signed. Microphone and Accessibility may need to be granted again after an update.",
    "更新没有开发者签名。更新后可能需要重新允许麦克风和辅助功能。",
  ));
  const steps = document.querySelector("#install-steps");
  if (steps) {
    const items = [
      ui(
        "macOS: drag newtype to Applications. Do not double-click the first time. Right-click, then Open. If a double-click was blocked, use System Settings, Privacy & Security, Open Anyway.",
        "macOS：把 newtype 拖到应用程序。第一次不要双击。右键，然后打开。如果双击被拦截，到系统设置的隐私与安全性里选择仍要打开。",
      ),
      ui(
        "Windows: if the system safety prompt appears, choose More info, then Run anyway.",
        "Windows：如果出现系统安全提示，选择更多信息，然后仍要运行。",
      ),
      ui(
        "The dictate key defaults to Right Ctrl. If this keyboard has no Right Ctrl key, use Hotkey below: click Capture, press a key that exists, then click Set.",
        "口授键默认是右 Ctrl。如果键盘没有右 Ctrl，用下面的快捷键：点击捕获，按一个存在的键，再点击设置。",
      ),
      ui(
        "A new install uses Groq: whisper-large-v3-turbo for speech and openai/gpt-oss-20b for rewrite, about $1.8/month at 200 dictations a day. An install that already has a provider keeps it.",
        "新安装默认用 Groq：语音 whisper-large-v3-turbo，改写 openai/gpt-oss-20b，每天约 200 次大约 $1.8/月。已经选过供应商的安装保持不变。",
      ),
    ];
    steps.innerHTML = items.map((item) => `<li>${item}</li>`).join("");
  }
}

function paint(): void {
  const host = document.querySelector("#dictate");
  if (!host) {
    return;
  }
  applyLanguage(snap.settings.uiLanguage || "auto", snap.language || "");
  refreshChrome();
  syncEditorsFromSnapshot();
  const settings = snap.settings;
  const asr = preset(settings.asrProvider);
  const llm = preset(settings.llmProvider);
  host.innerHTML = `
    <nav class="tabs" role="tablist" aria-label="${ui("Sections", "分区")}">
      <button type="button" class="tab${mainTab === "history" ? " active" : ""}" data-tab="history" role="tab" aria-selected="${mainTab === "history"}">${ui("History", "历史")}</button>
      <button type="button" class="tab${mainTab === "settings" ? " active" : ""}" data-tab="settings" role="tab" aria-selected="${mainTab === "settings"}">${ui("Settings", "设置")}</button>
    </nav>
    <p class="status" id="notice">${escapeHtml(notice)}</p>
    <div id="panel-history" role="tabpanel" ${mainTab === "history" ? "" : "hidden"}>
      ${lastResultCard()}
      <section id="sec-history">
        <h2>${ui("History", "历史")}</h2>
        <p class="status">${ui("On this device only. Each entry shows the mode and the path. The latest 300 that are still inside the retention window are kept.", "只存在这台电脑上。每条显示模式和路径。保留期内最新的 300 条会被留下。")}</p>
        <label class="field">${ui("Keep history", "保留时长")}
          <select id="retention">
            ${retentionOptions(settings.historyRetention || "month")}
          </select>
        </label>
        <label class="field">${ui("Search", "搜索")}<input id="history-search" value="${escapeHtml(historyQuery)}" placeholder="${ui("Text, mode, or path", "文本、模式或路径")}" /></label>
        <ul class="history">${visibleHistory().map((entry) => historyRow(entry)).join("") || `<li>${ui("No matching history.", "没有匹配的历史。")}</li>`}</ul>
        <div class="row"><button type="button" id="clear-history">${ui("Clear all", "全部清除")}</button></div>
      </section>
    </div>
    <div id="panel-settings" role="tabpanel" ${mainTab === "settings" ? "" : "hidden"}>
    ${windowSection(settings)}
    <section>
      <h2 id="sec-path">${ui("Path", "路径")}</h2>
      <p class="banner" id="route-banner">${escapeHtml(snap.routeLabel)}</p>
      <p class="status" id="connectivity">${escapeHtml(snap.connectivity)}</p>
      <p class="status" id="route-detail">${escapeHtml(snap.routeDetail)}</p>
      <p class="status" id="route-notice" ${snap.routeNotice ? "" : "hidden"}>${escapeHtml(snap.routeNotice)}</p>
      <p class="status">${ui("Hotkey listener", "快捷键监听")}：${escapeHtml(snap.listener)}</p>
      <p class="status">${ui("Log file", "日志文件")}：${escapeHtml(snap.logPath || ui("not open yet", "尚未打开"))}</p>
      ${snap.secureStore === "ready" ? "" : `<p class="status warn">${ui("Credential store", "凭据存储")}：${escapeHtml(snap.secureStore)}</p>`}
      ${radios("preference", [
        ["cloudDefault", ui("Auto", "自动"), ui("Cloud when the provider answers. If it stays unreachable, use local models, then return to cloud after the connection is stable.", "供应商能连通时走云端。一直连不上就用本地模型，连接稳定后再回到云端。")],
        ["cloudOnly", ui("Cloud only", "仅云端"), ui("Always the configured provider. A failure does not use local models.", "始终使用配置的供应商。失败时不用本地模型。")],
        ["localOnly", ui("Local only", "仅本地"), ui("Always on this computer. Coming back online does not switch to cloud.", "始终在这台电脑上。恢复联网也不会改走云端。")],
        ["strictOffline", ui("Strict offline", "严格离线"), ui("On this computer, and no outbound network: no cloud calls, probes, update checks, or model downloads.", "在这台电脑上，并且没有出站网络：没有云端调用、探测、更新检查或模型下载。")],
      ], settings.preference)}
    </section>
    ${localPanel(snap.local, settings.preference)}
    <section>
      <h2 id="sec-permissions">${ui("Permissions", "权限")}</h2>
      ${snap.permissions.prompt ? `<p class="banner warn" id="permission-prompt">${escapeHtml(snap.permissions.prompt)}</p><div class="row">${promptButtons(snap.permissions.promptAction)}</div>` : ""}
      <p class="status">${escapeHtml(snap.permissions.guidance)}</p>
      <p class="status">${ui("Microphone", "麦克风")}：${escapeHtml(snap.permissions.microphone)} · ${ui("Accessibility", "辅助功能")}：${escapeHtml(snap.permissions.accessibility)}</p>
      <div class="row">
        <button type="button" id="perm-mic" class="primary">${ui("Allow microphone", "允许麦克风")}</button>
        <button type="button" id="perm-ax">${ui("Allow Accessibility", "允许辅助功能")}</button>
        <button type="button" id="open-mic">${ui("Microphone settings", "麦克风设置")}</button>
        <button type="button" id="open-ax">${ui("Accessibility settings", "辅助功能设置")}</button>
        <button type="button" id="open-input">${ui("Input Monitoring", "输入监视")}</button>
      </div>
    </section>
    <section>
      <h2 id="sec-cloud">${ui("Cloud", "云端")}</h2>
      <p class="status">${ui("Cloud dictation sends the recording and the transcript to the provider you configure. The key stays on this computer.", "使用云端时会把录音和转写发到你配置的供应商。密钥只留在这台电脑上。")}</p>
      <label class="field">${ui("Preset", "方案")}
        <select id="cloud-preset">
          <option value="" disabled ${activeBundle(settings) ? "" : "selected"}>${ui("Custom mix", "自定义组合")}</option>
          ${snap.bundles.map((bundle) => `<option value="${escapeHtml(bundle.id)}" ${bundle.id === activeBundle(settings)?.id ? "selected" : ""}>${escapeHtml(bundle.label)} · ~$${escapeHtml(bundle.monthlyUsd)}${ui("/mo", "/月")}</option>`).join("")}
        </select>
      </label>
      <p class="status">${presetCostLine(settings)}</p>
      ${groqFallbackControl(settings)}
      ${keyStatus(settings)}
      <p class="status">${ui("Model ids were checked", "模型编号核对于")} ${escapeHtml(snap.verifiedOn)}. ${ui("The fields stay editable. Load models asks the provider what this key can see.", "字段仍可编辑。加载模型会询问这个密钥能看到哪些模型。")}</p>
      ${snap.devKeyStore ? `<p class="status warn">${ui("This machine is not macOS or Windows. The key is kept in a local dev file, not the OS keychain.", "这台电脑不是 macOS 或 Windows。密钥存在本地开发文件里，不在系统钥匙串。")}</p>` : `<p class="status">${ui("API keys are stored in the macOS Keychain or Windows Credential Manager. They are not written to settings.", "密钥存在 macOS 钥匙串或 Windows 凭据管理器里，不会写入设置。")}</p>`}
      <label class="field">${ui("Speech", "语音")}<select id="asr-provider">${options(snap.presets, settings.asrProvider, (item) => item.asrReady)}</select></label>
      <p class="status">${badge(asr)} ${escapeHtml(asr?.note ?? "")}</p>
      <label class="field">${ui("Rewrite", "改写")}<select id="llm-provider">${options(snap.presets, settings.llmProvider, (item) => item.llmReady)}</select></label>
      <p class="status">${badge(llm)} ${escapeHtml(llm?.note ?? "")}</p>
      ${providerFields(settings)}
      <label class="field">${ui("Thinking", "思考")}
        <select id="thinking">
          <option value="auto" ${settings.thinking === "auto" ? "selected" : ""}>${ui("Auto (off where the model allows it)", "自动（模型允许时关闭）")}</option>
          <option value="off" ${settings.thinking === "off" ? "selected" : ""}>${ui("Force off", "强制关闭")}</option>
          <option value="low" ${settings.thinking === "low" ? "selected" : ""}>${ui("Low effort when off is rejected", "关闭被拒绝时用低强度")}</option>
        </select>
      </label>
      <p class="status">${ui("Speech key", "语音密钥")} (${escapeHtml(preset(settings.asrProvider)?.label ?? settings.asrProvider)}): ${snap.keys[settings.asrProvider] ? ui("saved", "已保存") : ui("missing", "缺少")}</p>
      <label class="field">${ui("API key for", "密钥")} ${escapeHtml(settings.asrProvider)}
        <input id="api-key" type="password" autocomplete="off" placeholder="${snap.keys[settings.asrProvider] ? ui("Saved. Enter a new key to replace it.", "已保存。输入新密钥以替换。") : ui("Not saved", "未保存")}" />
      </label>
      ${settings.llmProvider !== settings.asrProvider ? `<p class="status">${ui("Rewrite key", "改写密钥")} (${escapeHtml(preset(settings.llmProvider)?.label ?? settings.llmProvider)}): ${snap.keys[settings.llmProvider] ? ui("saved", "已保存") : ui("missing", "缺少")}</p><label class="field">${ui("API key for", "密钥")} ${escapeHtml(settings.llmProvider)}<input id="llm-key" type="password" autocomplete="off" placeholder="${snap.keys[settings.llmProvider] ? ui("Saved. Enter a new key to replace it.", "已保存。输入新密钥以替换。") : ui("Not saved", "未保存")}" /></label>` : ""}
      <div class="row">
        <button type="button" id="save-key" class="primary">${ui("Save key", "保存密钥")}</button>
        <button type="button" id="clear-key">${ui("Clear speech key", "清除语音密钥")}</button>
        <button type="button" id="test-key" data-kind="speech">${ui("Test speech connection", "测试语音连接")}</button>
        <button type="button" id="test-llm" data-kind="rewrite">${ui("Test rewrite connection", "测试改写连接")}</button>
        <button type="button" id="load-models">${ui("Load models", "加载模型")}</button>
      </div>
      <div class="row">
        ${snap.presets.map((item) => `<button type="button" data-test-provider="${escapeHtml(item.id)}">${ui("Test connection", "测试连接")} · ${escapeHtml(item.label)}</button>`).join("")}
      </div>
    </section>
    <section>
      <h2 id="sec-hotkey">${ui("Hotkeys", "快捷键")}</h2>
      <p class="status hotkey-hint">${ui("The default chord uses Right Ctrl. If this keyboard has no Right Ctrl key, click Capture, press a key that exists (Right Alt works), then click Set. Set saves that chord and starts using it immediately. Cancel or Escape leaves the saved chord in place. A Typeless clash is a warning, and Set still keeps the chord.", "默认组合是右 Ctrl。如果键盘没有右 Ctrl，点击捕获，按一个存在的键（右 Alt 可以），再点击设置。设置会立刻保存并使用。取消或 Esc 保留原来的组合。和 Typeless 冲突只是警告，设置仍会保留这个组合。")}</p>
      ${radios("talk", [["toggle", ui("Toggle", "切换"), ui("Press to start, press again to stop.", "按一次开始，再按一次停止。")], ["hold", ui("Hold to talk", "按住说话"), ui("Release to stop.", "松开即停止。")]], settings.talkMode)}
      <div id="hotkey-editor">${hotkeyEditorInner()}</div>
    </section>
    <section>
      <h2 id="sec-polish">${ui("Polish", "润色")}</h2>
      ${radios("polish", [["polish", ui("Polish", "润色"), ui("Smoother wording. Does not answer or carry out the request.", "语句更顺。不会回答或执行请求。")], ["literal", ui("Literal", "逐字"), ui("Minimal cleanup. Wording stays close to the transcript.", "只做少量整理。用词接近转写。")]], settings.polishLevel)}
    </section>
    <section id="sec-language">
      <h2>${ui("Language", "语言")}</h2>
      <label class="field">${ui("Language", "语言")}
        <select id="ui-language">
          <option value="auto" ${(settings.uiLanguage || "auto") === "auto" ? "selected" : ""}>Auto</option>
          <option value="zh" ${settings.uiLanguage === "zh" ? "selected" : ""}>中文</option>
          <option value="en" ${settings.uiLanguage === "en" ? "selected" : ""}>English</option>
        </select>
      </label>
      <p class="status">${ui("Auto follows the system language. Translation has one target.", "自动跟随系统语言。翻译使用一个目标语言。")}</p>
      <label class="field">${ui("Translation target", "翻译目标语言")}
        <input id="target-language" list="target-languages" maxlength="40" value="${escapeHtml(settings.targetLanguage || "English")}" />
      </label>
      <datalist id="target-languages">${TARGET_LANGUAGES.map((language) => `<option value="${escapeHtml(language)}"></option>`).join("")}</datalist>
    </section>
    <section id="sec-dictionary">
      <h2>${ui("Dictionary", "词典")}</h2>
      <p class="status">${ui("Terms are sent as a speech hint and listed in the rewrite prompt. Replacements run before the model. A term with no replacement is prompt-only.", "词条会作为语音提示，并写进改写提示。替换在模型之前执行。没有替换的词条只出现在提示里。")}</p>
      <label class="field">${ui("Search", "搜索")}<input id="dictionary-search" value="${escapeHtml(dictionaryQuery)}" placeholder="${ui("Term or replacement", "词条或替换")}" /></label>
      <div id="hotwords">${settings.hotwords.map((word, index) => hotwordRow(word, index)).join("")}</div>
      <div class="row">
        <button type="button" id="add-hotword">${ui("Add term", "添加词条")}</button>
        <button type="button" id="export-dictionary">${ui("Export JSON", "导出 JSON")}</button>
        <button type="button" id="import-dictionary">${ui("Import dictionary", "导入词典")}</button>
      </div>
      <p class="status">${ui("Import merges. JSON v1, one term per line, or CSV: term,replacement1|replacement2. A term already saved with the same spelling is kept once.", "导入会合并。支持 JSON v1、每行一个词条，或 CSV：词条,替换1|替换2。拼写完全相同的词条只保留一条。")}</p>
      <input id="dictionary-file" type="file" accept="application/json,.json,text/plain,.txt,text/csv,.csv" hidden />
    </section>
    <section>
      <h2 id="sec-audio">${ui("Audio", "语音")}</h2>
      <label class="field">${ui("Device", "设备")}<input id="microphone" list="mic-list" value="${escapeHtml(settings.microphone)}" placeholder="${ui("System default", "系统默认")}" /></label>
      <datalist id="mic-list"></datalist>
      <div class="row"><button type="button" id="refresh-mics">${ui("List devices", "列出设备")}</button><button type="button" id="probe-mic">${ui("Test microphone", "测试麦克风")}</button><button type="button" id="record">${ui("Record a test", "试录一段")}</button></div>
    </section>
    <section>
      <div class="row"><button type="button" id="save" class="primary">${ui("Save settings", "保存设置")}</button><button type="button" id="reset-models">${ui("Reset suggested models", "恢复建议模型")}</button></div>
    </section>
    </div>
  `;
  const about = document.querySelector("#about-licenses");
  if (about && snap.local.attribution) {
    about.textContent = snap.local.attribution;
  }
  bind();
  bindLocal(preview, refreshLocal);
  syncTranslucency();
}

function bind(): void {
  document.querySelector("#groq-openai-fallback")?.addEventListener("change", (event) => {
    const groqRateLimitFallback = (event.target as HTMLInputElement).checked;
    if (groqRateLimitFallback === snap.settings.groqRateLimitFallback) {
      return;
    }
    snap.settings.groqRateLimitFallback = groqRateLimitFallback;
    void persist({ groqRateLimitFallback });
  });
  document.querySelector("#cloud-preset")?.addEventListener("change", (event) => {
    const id = (event.target as HTMLSelectElement).value;
    if (id) {
      applyBundle(id);
    }
  });
  document.querySelector("#asr-provider")?.addEventListener("change", (event) => {
    snap.settings.asrProvider = (event.target as HTMLSelectElement).value;
    paint();
  });
  document.querySelector("#llm-provider")?.addEventListener("change", (event) => {
    snap.settings.llmProvider = (event.target as HTMLSelectElement).value;
    paint();
  });
  document.querySelector("#thinking")?.addEventListener("change", (event) => {
    snap.settings.thinking = (event.target as HTMLSelectElement).value;
  });
  for (const input of document.querySelectorAll<HTMLInputElement>('input[name="preference"]')) {
    input.addEventListener("change", () => {
      if (!input.checked || input.value === snap.settings.preference) {
        return;
      }
      snap.settings.preference = input.value;
      void persist({ preference: input.value });
    });
  }
  for (const input of document.querySelectorAll<HTMLInputElement>('input[name="talk"]')) {
    input.addEventListener("change", () => {
      if (input.checked) {
        snap.settings.talkMode = input.value;
      }
    });
  }
  for (const input of document.querySelectorAll<HTMLInputElement>('input[name="polish"]')) {
    input.addEventListener("change", () => {
      if (input.checked) {
        snap.settings.polishLevel = input.value;
      }
    });
  }
  document.querySelector("#save")?.addEventListener("click", () => void save());
  document.querySelector("#window-translucent")?.addEventListener("change", (event) => {
    const windowTranslucent = (event.target as HTMLInputElement).checked;
    if (windowTranslucent === snap.settings.windowTranslucent) {
      return;
    }
    snap.settings.windowTranslucent = windowTranslucent;
    syncTranslucency();
    void persist({ windowTranslucent });
  });
  document.querySelector("#indicator-place")?.addEventListener("change", (event) => {
    const indicatorPlace = (event.target as HTMLSelectElement).value || "taskbar";
    if (indicatorPlace === (snap.settings.indicatorPlace || "taskbar")) {
      return;
    }
    snap.settings.indicatorPlace = indicatorPlace;
    void persist({ indicatorPlace });
  });
  document.querySelector("#ui-language")?.addEventListener("change", (event) => {
    const uiLanguage = (event.target as HTMLSelectElement).value || "auto";
    if (uiLanguage === (snap.settings.uiLanguage || "auto")) {
      return;
    }
    snap.settings.uiLanguage = uiLanguage;
    applyLanguage(uiLanguage, snap.language || "");
    void persist({ uiLanguage });
  });
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-tab]")) {
    button.addEventListener("click", () => {
      const next = button.dataset.tab;
      if (next === "history" || next === "settings") {
        readForm();
        mainTab = next;
        paint();
      }
    });
  }
  document.querySelector("#save-key")?.addEventListener("click", () => void saveKeys());
  document.querySelector("#clear-key")?.addEventListener("click", () => void clearKey(snap.settings.asrProvider));
  document.querySelector("#test-key")?.addEventListener("click", () => void testKey(snap.settings.asrProvider, "speech"));
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-test-provider]")) {
    button.addEventListener("click", () => void testKey(button.dataset.testProvider ?? "", "models"));
  }
  document.querySelector("#test-llm")?.addEventListener("click", () => void testKey(snap.settings.llmProvider, "rewrite"));
  document.querySelector("#load-models")?.addEventListener("click", () => void loadModels(snap.settings.asrProvider));
  document.querySelector("#reset-models")?.addEventListener("click", () => void resetModels());
  document.querySelector("#add-hotword")?.addEventListener("click", () => {
    readHotwords();
    snap.settings.hotwords.push({ canonical: "", aliases: [], pattern: "" });
    paint();
  });
  document.querySelector("#dictionary-search")?.addEventListener("input", (event) => {
    readHotwords();
    dictionaryQuery = (event.target as HTMLInputElement).value;
    const host = document.querySelector("#hotwords");
    if (!host) {
      return;
    }
    host.innerHTML = snap.settings.hotwords.map((word, index) => hotwordRow(word, index)).join("");
    bindHotwordButtons();
  });
  document.querySelector("#export-dictionary")?.addEventListener("click", () => void exportDictionary());
  document.querySelector("#import-dictionary")?.addEventListener("click", () => {
    document.querySelector<HTMLInputElement>("#dictionary-file")?.click();
  });
  document.querySelector("#dictionary-file")?.addEventListener("change", (event) => {
    const file = (event.target as HTMLInputElement).files?.[0];
    if (file) {
      void importDictionary(file);
    }
  });
  document.querySelector("#history-search")?.addEventListener("input", (event) => {
    historyQuery = (event.target as HTMLInputElement).value;
    paint();
  });
  document.querySelector("#clear-history")?.addEventListener("click", () => void clearHistory());
  bindHotwordButtons();
  document.querySelector("#perm-mic")?.addEventListener("click", () => void permission("microphone"));
  document.querySelector("#perm-ax")?.addEventListener("click", () => void permission("accessibility"));
  document.querySelector("#open-mic")?.addEventListener("click", () => void openPrivacy("microphone"));
  document.querySelector("#open-ax")?.addEventListener("click", () => void openPrivacy("accessibility"));
  document.querySelector("#open-input")?.addEventListener("click", () => void openPrivacy("input"));
  document.querySelector("#prompt-mic")?.addEventListener("click", () => void openPrivacy("microphone"));
  document.querySelector("#prompt-ax")?.addEventListener("click", () => void openPrivacy("accessibility"));
  document.querySelector("#refresh-mics")?.addEventListener("click", () => void listMics());
  document.querySelector("#probe-mic")?.addEventListener("click", () => void probe());
  document.querySelector("#record")?.addEventListener("click", () => void record());
  bindHotkeyButtons();
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-copy]")) {
    button.addEventListener("click", () => void copyHistory(button.dataset.copy ?? ""));
  }
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-delete]")) {
    button.addEventListener("click", () => void deleteHistory(button.dataset.delete ?? ""));
  }
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-reinsert]")) {
    button.addEventListener("click", () => void reinsertHistory(button.dataset.reinsert ?? ""));
  }
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-rerun]")) {
    button.addEventListener("click", () => void rerunHistory(button.dataset.rerun ?? ""));
  }
}

function readForm(): void {
  const settings = snap.settings;
  const read = (id: string) => document.querySelector<HTMLInputElement>(id)?.value ?? "";
  settings.microphone = read("#microphone");
  const target = read("#target-language").trim();
  settings.targetLanguage = target || "English";
  settings.historyRetention = document.querySelector<HTMLSelectElement>("#retention")?.value || settings.historyRetention || "month";
  settings.uiLanguage = document.querySelector<HTMLSelectElement>("#ui-language")?.value || settings.uiLanguage || "auto";
  const translucent = document.querySelector<HTMLInputElement>("#window-translucent");
  if (translucent) {
    settings.windowTranslucent = translucent.checked;
  }
  const place = document.querySelector<HTMLSelectElement>("#indicator-place");
  if (place) {
    settings.indicatorPlace = place.value || settings.indicatorPlace || "taskbar";
  }
  const groqFallback = document.querySelector<HTMLInputElement>("#groq-openai-fallback");
  if (groqFallback) {
    settings.groqRateLimitFallback = groqFallback.checked;
  }
  settings.thinking = document.querySelector<HTMLSelectElement>("#thinking")?.value ?? settings.thinking;
  writeProvider(settings, settings.asrProvider, true);
  if (settings.llmProvider !== settings.asrProvider) {
    writeProvider(settings, settings.llmProvider, false);
  } else {
    writeProvider(settings, settings.llmProvider, true);
  }
  readHotwords();
}

function writeProvider(settings: DictationSettings, id: string, speech: boolean): void {
  const config = ensure(settings, id);
  const appId = document.querySelector<HTMLInputElement>("#volc-app-id");
  if (appId && id === "volcengine") {
    config.volcAppId = appId.value.trim();
  }
  if (speech) {
    const model = document.querySelector<HTMLInputElement>("#asr-model");
    if (model) {
      config.asrModel = model.value.trim();
    }
    const base = document.querySelector<HTMLInputElement>("#provider-base");
    if (base) {
      config.base = base.value.trim();
    }
  }
  if (!speech || settings.llmProvider === settings.asrProvider) {
    const model = document.querySelector<HTMLInputElement>("#llm-model");
    if (model && id === settings.llmProvider) {
      config.llmModel = model.value.trim();
    }
  }
  if (!speech) {
    const base = document.querySelector<HTMLInputElement>("#llm-provider-base");
    if (base) {
      config.base = base.value.trim();
    }
  }
  const asrBase = document.querySelector<HTMLInputElement>("#asr-base");
  const llmBase = document.querySelector<HTMLInputElement>("#llm-base");
  if (asrBase && id === "custom") {
    config.asrBase = asrBase.value.trim();
  }
  if (llmBase && id === "custom") {
    config.llmBase = llmBase.value.trim();
  }
  const wire = document.querySelector<HTMLSelectElement>("#asr-wire");
  if (wire && id === "custom") {
    config.asrWire = wire.value;
  }
  const thinking = document.querySelector<HTMLSelectElement>("#custom-thinking");
  if (thinking && id === "custom") {
    config.thinking = thinking.value;
  }
}

function readHotwords(): void {
  const rows = document.querySelectorAll<HTMLElement>(".hotword");
  const words: Hotword[] = [];
  rows.forEach((row) => {
    const canonical = row.querySelector<HTMLInputElement>(".canonical")?.value.trim() ?? "";
    const aliases = (row.querySelector<HTMLInputElement>(".aliases")?.value ?? "")
      .split(",")
      .map((alias) => alias.trim())
      .filter(Boolean);
    const pattern = row.querySelector<HTMLInputElement>(".pattern")?.value.trim() ?? "";
    if (canonical || aliases.length || pattern) {
      words.push({ canonical, aliases, pattern });
    }
  });
  snap.settings.hotwords = words;
}

function formPatch(): Record<string, unknown> {
  const settings = snap.settings;
  return {
    asrProvider: settings.asrProvider,
    llmProvider: settings.llmProvider,
    talkMode: settings.talkMode,
    polishLevel: settings.polishLevel,
    preference: settings.preference,
    thinking: settings.thinking,
    microphone: settings.microphone,
    localMirror: settings.localMirror,
    localDownloadSkipped: settings.localDownloadSkipped,
    localAsrId: settings.localAsrId,
    localLlmId: settings.localLlmId,
    localIdleMinutes: settings.localIdleMinutes,
    targetLanguage: settings.targetLanguage,
    historyRetention: settings.historyRetention,
    windowTranslucent: settings.windowTranslucent,
    indicatorPlace: settings.indicatorPlace,
    groqRateLimitFallback: settings.groqRateLimitFallback,
    uiLanguage: settings.uiLanguage,
    providers: settings.providers,
    hotwords: settings.hotwords,
  };
}

async function save(): Promise<void> {
  readForm();
  await persist(formPatch());
}

async function persist(patch: Record<string, unknown>): Promise<void> {
  if (preview) {
    notice = "Preview only. Settings are not saved.";
    paint();
    return;
  }
  try {
    snap = await call<Snapshot>("dictation_save", { settings: patch });
    applyLanguage(snap.settings.uiLanguage || "auto", snap.language || "");
    notice = ui("Saved.", "已保存。");
    paint();
  } catch (error) {
    notice = String(error);
    const node = document.querySelector("#notice");
    if (node) {
      node.textContent = notice;
    }
  }
}

async function saveKeys(): Promise<void> {
  const speech = document.querySelector<HTMLInputElement>("#api-key")?.value ?? "";
  const rewrite = document.querySelector<HTMLInputElement>("#llm-key")?.value ?? "";
  const volcSpeech = document.querySelector<HTMLInputElement>("#volc-speech-key")?.value ?? "";
  const volcToken = document.querySelector<HTMLInputElement>("#volc-token")?.value ?? "";
  if (preview) {
    if (speech.trim()) {
      snap.keys[snap.settings.asrProvider] = true;
    }
    notice = "Preview only. The key was not stored.";
    paint();
    return;
  }
  if (!speech.trim() && !rewrite.trim() && !volcSpeech.trim() && !volcToken.trim()) {
    notice = "Enter an API key before saving.";
    const node = document.querySelector("#notice");
    if (node) {
      node.textContent = notice;
    }
    return;
  }
  try {
    const messages: string[] = [];
    if (speech.trim()) {
      messages.push(await call<string>("dictation_set_key", { provider: snap.settings.asrProvider, key: speech }));
    }
    if (rewrite.trim()) {
      messages.push(await call<string>("dictation_set_key", { provider: snap.settings.llmProvider, key: rewrite }));
    }
    if (volcSpeech.trim()) {
      messages.push(await call<string>("dictation_set_key", { provider: "volcengine-speech", key: volcSpeech }));
    }
    if (volcToken.trim()) {
      messages.push(await call<string>("dictation_set_key", { provider: "volcengine-token", key: volcToken }));
    }
    await reload();
    notice = messages.join(" ");
    const node = document.querySelector("#notice");
    if (node) {
      node.textContent = notice;
    }
  } catch (error) {
    notice = String(error);
    const node = document.querySelector("#notice");
    if (node) {
      node.textContent = notice;
    }
  }
}

async function clearKey(provider: string): Promise<void> {
  if (preview) {
    snap.keys[provider] = false;
    notice = "Preview key cleared.";
    paint();
    return;
  }
  await call("dictation_clear_key", { provider });
  notice = "Key removed.";
  await reload();
}

async function testKey(provider: string, kind: string): Promise<void> {
  if (preview) {
    notice = ui("Preview cannot reach a provider.", "预览无法连接供应商。");
    paint();
    return;
  }
  try {
    notice = await call<string>("dictation_test_connection", { provider, kind });
  } catch (error) {
    notice = String(error);
  }
  const node = document.querySelector("#notice");
  if (node) {
    node.textContent = notice;
  }
}

async function loadModels(provider: string): Promise<void> {
  if (preview) {
    remoteModels = ["whisper-large-v3-turbo", "openai/gpt-oss-20b", "gpt-transcribe", "gpt-4o-mini"];
    notice = "Preview model list.";
    paint();
    return;
  }
  try {
    remoteModels = await call<string[]>("dictation_list_models", { provider });
    notice = remoteModels.length ? `Loaded ${remoteModels.length} model ids.` : "The provider returned no model ids. You can still type one.";
    paint();
  } catch (error) {
    notice = String(error);
    const node = document.querySelector("#notice");
    if (node) {
      node.textContent = notice;
    }
  }
}

async function resetModels(): Promise<void> {
  if (preview) {
    notice = "Preview keeps the suggested ids.";
    paint();
    return;
  }
  snap = await call<Snapshot>("dictation_reset_models");
  notice = "Suggested models restored. Keys were not changed.";
  paint();
}

async function permission(kind: string): Promise<void> {
  if (preview) {
    notice = kind === "microphone" ? "Preview cannot open the microphone." : "Preview cannot prompt for Accessibility.";
    paint();
    return;
  }
  try {
    notice = await call<string>("dictation_request_access", { kind });
  } catch (error) {
    notice = String(error);
  }
  await reload();
  const node = document.querySelector("#notice");
  if (node) {
    node.textContent = notice;
  }
}

async function openPrivacy(kind: string): Promise<void> {
  if (preview) {
    notice = "Preview cannot open System Settings.";
    paint();
    return;
  }
  try {
    await call("dictation_open_privacy", { kind });
  } catch (error) {
    notice = String(error);
    const node = document.querySelector("#notice");
    if (node) {
      node.textContent = notice;
    }
  }
}

async function listMics(): Promise<void> {
  const list = document.querySelector("#mic-list");
  if (!list) {
    return;
  }
  const names = preview ? ["MacBook Microphone"] : await call<string[]>("dictation_microphones");
  list.innerHTML = names.map((name) => `<option value="${escapeHtml(name)}"></option>`).join("");
  notice = names.length ? names.join(", ") : "No input devices reported.";
  const node = document.querySelector("#notice");
  if (node) {
    node.textContent = notice;
  }
}

async function probe(): Promise<void> {
  readForm();
  if (preview) {
    notice = "Preview cannot hear the microphone.";
    paint();
    return;
  }
  try {
    notice = await call<string>("dictation_probe_microphone", { device: snap.settings.microphone });
  } catch (error) {
    notice = String(error);
  }
  const node = document.querySelector("#notice");
  if (node) {
    node.textContent = notice;
  }
}

async function record(): Promise<void> {
  if (preview) {
    notice = "Preview cannot record.";
    paint();
    return;
  }
  await call("dictation_record_toggle");
}

async function capture(mode: HotkeyModeName): Promise<void> {
  for (const [other] of HOTKEY_MODES) {
    if (other !== mode && editors[other].phase === "capturing") {
      editors[other] = cancelEditor(editors[other]);
    }
  }
  const ticket = ++captureTicket;
  editors[mode] = beginCapture(editors[mode]);
  setHotkeyMessage(
    preview
      ? "Preview has no keyboard hook. Choose the chord, then click Set."
      : "Press the keys, then release them. Escape cancels.",
  );
  renderHotkeys();
  if (preview) {
    return;
  }
  window.setTimeout(() => {
    if (ticket !== captureTicket) {
      return;
    }
    const editor = editors[mode];
    if (!captureHeardNothing(editor.phase, editor.pendingLabel, CAPTURE_SILENCE_MS)) {
      return;
    }
    setHotkeyMessage(
      ui(
        "No key arrived. The keyboard hook may not be hearing keys. Choose a dictate key from the list, or try Capture again.",
        "没有收到按键。键盘钩子可能没有听到按键。请从列表里选口授键，或再试一次捕获。",
      ),
      true,
    );
    renderHotkeys();
  }, CAPTURE_SILENCE_MS);
  try {
    await call("dictation_capture_hotkey", { mode });
  } catch (error) {
    editors[mode] = cancelEditor(editors[mode]);
    setHotkeyMessage(String(error), true);
    renderHotkeys();
    await call("dictation_cancel_capture").catch(() => undefined);
  }
}

function acceptPreviewChord(mode: HotkeyModeName, chord: string, label: string): void {
  editors[mode] = showCaptured(editors[mode], chord, label);
  const warning = knownConflict(chord);
  setHotkeyMessage(
    warning
      ? `${label} captured. ${warning} Click Set to keep it.`
      : `${label} captured. Click Set to keep it.`,
  );
  renderHotkeys();
}

async function setHotkey(mode: HotkeyModeName): Promise<void> {
  const editor = editors[mode];
  if (editor.phase !== "pending" || !editor.pendingChord) {
    setHotkeyMessage("Capture a chord before Set.", true);
    renderHotkeys();
    return;
  }
  const chords = HOTKEY_MODES.map(([name], index) =>
    name === mode ? editor.pendingChord : editors[name].activeChord || snap.hotkeyChords[index] || "",
  );
  const localError = bindingError(chords);
  if (localError) {
    setHotkeyMessage(localError, true);
    renderHotkeys();
    return;
  }
  const found = HOTKEY_MODES.find(([name]) => name === mode);
  const title = found ? ui(found[1], found[2]) : mode;
  if (preview) {
    snap.settings.dictateHotkey = chords[0] ?? "";
    snap.settings.translateHotkey = chords[1] ?? "";
    snap.settings.commandHotkey = chords[2] ?? "";
    snap.hotkeyChords = chords;
    snap.hotkeyLabels = HOTKEY_MODES.map(([name], index) =>
      name === mode ? editor.pendingLabel : snap.hotkeyLabels[index] ?? "",
    );
    snap.warnings = bindingWarnings(chords);
    editors[mode] = idleEditor(editor.pendingChord, editor.pendingLabel);
    syncEditorsFromSnapshot();
    setHotkeyMessage(appliedMessage(title, editor.pendingLabel, snap.warnings));
    renderHotkeys();
    return;
  }
  try {
    const next = await call<Snapshot>("dictation_set_hotkey", { mode, hotkey: editor.pendingChord });
    const index = HOTKEY_MODES.findIndex(([name]) => name === mode);
    const label = next.hotkeyLabels[index] ?? editor.pendingLabel;
    snap.hotkeyLabels = next.hotkeyLabels;
    snap.hotkeyChords = next.hotkeyChords;
    snap.warnings = next.warnings;
    snap.settings.dictateHotkey = next.settings.dictateHotkey;
    snap.settings.translateHotkey = next.settings.translateHotkey;
    snap.settings.commandHotkey = next.settings.commandHotkey;
    editors[mode] = idleEditor(next.hotkeyChords[index] ?? editor.pendingChord, label);
    syncEditorsFromSnapshot();
    setHotkeyMessage(appliedMessage(title, label, next.warnings));
    renderHotkeys();
  } catch (error) {
    setHotkeyMessage(String(error), true);
    renderHotkeys();
  }
}

async function cancelHotkey(mode: HotkeyModeName): Promise<void> {
  const wasCapturing = editors[mode].phase === "capturing";
  editors[mode] = cancelEditor(editors[mode]);
  const someoneCapturing = HOTKEY_MODES.some(([name]) => editors[name].phase === "capturing");
  setHotkeyMessage("Capture cancelled. The saved hotkey is unchanged.");
  renderHotkeys();
  if (!preview && (wasCapturing || !someoneCapturing)) {
    await call("dictation_cancel_capture");
  }
}

function appliedMessage(title: string, label: string, warnings: string[]): string {
  const typeless = warnings.filter((warning) => warning.includes("Typeless"));
  return typeless.length ? `Set ${title} to ${label}. ${typeless.join(" ")}` : `Set ${title} to ${label}.`;
}

function setHotkeyMessage(message: string, isError = false): void {
  hotkeyMessage = message;
  hotkeyMessageError = isError;
  notice = message;
  setNotice(message, isError);
}

function setNotice(message: string, isError = false): void {
  const node = document.querySelector("#notice");
  if (!node) {
    return;
  }
  node.textContent = message;
  node.classList.toggle("error", isError);
}

function syncEditorsFromSnapshot(): void {
  HOTKEY_MODES.forEach(([mode], index) => {
    editors[mode].activeChord = snap.hotkeyChords[index] ?? editors[mode].activeChord;
    editors[mode].activeLabel = snap.hotkeyLabels[index] ?? editors[mode].activeLabel;
  });
}

function renderHotkeys(): void {
  const host = document.querySelector("#hotkey-editor");
  if (!host) {
    return;
  }
  host.innerHTML = hotkeyEditorInner();
  bindHotkeyButtons();
}

function bindHotkeyButtons(): void {
  document.querySelector("#defaults")?.addEventListener("click", () => {
    for (const [mode] of HOTKEY_MODES) {
      editors[mode] = cancelEditor(editors[mode]);
    }
    snap.settings.dictateHotkey = "";
    snap.settings.translateHotkey = "";
    snap.settings.commandHotkey = "";
    if (preview) {
      snap.hotkeyChords = [...previewSnapshot.hotkeyChords];
      snap.hotkeyLabels = [...previewSnapshot.hotkeyLabels];
      snap.warnings = [...previewSnapshot.warnings];
    }
    setHotkeyMessage("Restored the default hotkeys. Dictate is Right Ctrl.");
    void persist({
      clearHotkeys: true,
      dictateHotkey: "",
      translateHotkey: "",
      commandHotkey: "",
    });
  });
  document.querySelector<HTMLSelectElement>("[data-dictate-key]")?.addEventListener("change", (event) => {
    const chord = (event.target as HTMLSelectElement).value;
    if (!chord) {
      return;
    }
    void chooseHotkey("dictate", chord);
  });
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-capture]")) {
    button.addEventListener("click", () => void capture((button.dataset.capture ?? "dictate") as HotkeyModeName));
  }
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-set]")) {
    button.addEventListener("click", () => void setHotkey((button.dataset.set ?? "dictate") as HotkeyModeName));
  }
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-cancel]")) {
    button.addEventListener("click", () => void cancelHotkey((button.dataset.cancel ?? "dictate") as HotkeyModeName));
  }
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-preview-chord]")) {
    button.addEventListener("click", () => {
      const mode = (button.dataset.mode ?? "dictate") as HotkeyModeName;
      acceptPreviewChord(mode, button.dataset.previewChord ?? "", button.dataset.previewLabel ?? "");
    });
  }
}

function hotkeyEditorInner(): string {
  const rows = HOTKEY_MODES.map(([mode, en, zh], index) => hotkeyRow(mode, ui(en, zh), index)).join("");
  const notes = snap.warnings.map((warning) => `<li>${escapeHtml(warning)}</li>`).join("");
  const status = hotkeyMessage
    ? `<p class="status${hotkeyMessageError ? " error" : ""}" id="hotkey-status">${escapeHtml(hotkeyMessage)}</p>`
    : "";
  return `${rows}${status}<div class="row"><button type="button" id="defaults">Restore default hotkeys</button></div><ul class="notes" id="hotkey-notes">${notes}</ul>`;
}

async function exportDictionary(): Promise<void> {
  readHotwords();
  if (preview) {
    downloadText(
      "newtype-dictionary.json",
      JSON.stringify(
        {
          version: 1,
          terms: snap.settings.hotwords.map((word) => ({
            term: word.canonical,
            replacements: word.aliases,
            pattern: word.pattern,
          })),
        },
        null,
        2,
      ),
    );
    return;
  }
  try {
    const body = await call<string>("dictation_export_dictionary");
    downloadText("newtype-dictionary.json", body);
  } catch (error) {
    notice = String(error);
    paint();
  }
}

async function importDictionary(file: File): Promise<void> {
  const body = await file.text();
  if (preview) {
    try {
      const parsed = JSON.parse(body) as { terms?: Hotword[] } | Hotword[];
      const terms = Array.isArray(parsed) ? parsed : parsed.terms ?? [];
      snap.settings.hotwords = terms.map((term) => ({
        canonical: String((term as { term?: string; canonical?: string }).term ?? (term as { canonical?: string }).canonical ?? ""),
        aliases: aliasesOf(term),
        pattern: String((term as { pattern?: string }).pattern ?? ""),
      }));
      notice = ui("Imported the dictionary in preview.", "预览里已导入词典。");
    } catch (error) {
      notice = String(error);
    }
    paint();
    return;
  }
  try {
    snap = await call<Snapshot>("dictation_import_dictionary", { body });
    notice = ui("Imported the dictionary.", "已导入词典。");
    paint();
  } catch (error) {
    notice = String(error);
    const node = document.querySelector("#notice");
    if (node) {
      node.textContent = notice;
    }
  }
}

function aliasesOf(term: unknown): string[] {
  const record = term as { replacements?: unknown; aliases?: unknown };
  const value = record.replacements ?? record.aliases;
  if (Array.isArray(value)) {
    return value.map((item) => String(item)).filter(Boolean);
  }
  if (typeof value === "string") {
    return value.split(",").map((item) => item.trim()).filter(Boolean);
  }
  return [];
}

function downloadText(name: string, body: string): void {
  const blob = new Blob([body], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.click();
  URL.revokeObjectURL(url);
}

async function clearHistory(): Promise<void> {
  if (!window.confirm(ui("Delete every history entry on this computer?", "删除这台电脑上的全部历史？"))) {
    return;
  }
  if (preview) {
    history = [];
    paint();
    return;
  }
  await call("dictation_clear_history");
  history = [];
  paint();
}

async function reinsertHistory(id: string): Promise<void> {
  if (preview) {
    notice = ui("Preview does not paste into another app.", "预览不会粘贴到其他应用。");
    paint();
    return;
  }
  try {
    await call("dictation_reinsert_history", { id });
    notice = ui("Inserted again.", "已再次插入。");
  } catch (error) {
    notice = String(error);
  }
  const node = document.querySelector("#notice");
  if (node) {
    node.textContent = notice;
  }
}

async function rerunHistory(id: string): Promise<void> {
  if (preview) {
    notice = ui("Preview cannot re-run a recording.", "预览不能重新识别录音。");
    paint();
    return;
  }
  notice = ui("Re-running the saved recording…", "正在重新识别保存的录音…");
  const node = document.querySelector("#notice");
  if (node) {
    node.textContent = notice;
  }
  try {
    notice = await call<string>("dictation_rerun_history", { id });
    history = await call<HistoryEntry[]>("dictation_history");
  } catch (error) {
    notice = String(error);
  }
  paint();
}

async function copyHistory(id: string): Promise<void> {
  if (preview) {
    notice = "Preview copied nothing to the system clipboard.";
    const node = document.querySelector("#notice");
    if (node) {
      node.textContent = notice;
    }
    return;
  }
  await call("dictation_copy_history", { id });
  notice = "Copied.";
  const node = document.querySelector("#notice");
  if (node) {
    node.textContent = notice;
  }
}

async function deleteHistory(id: string): Promise<void> {
  if (preview) {
    history = history.filter((entry) => entry.id !== id);
    paint();
    return;
  }
  await call("dictation_delete_history", { id });
  history = await call<HistoryEntry[]>("dictation_history");
  paint();
}

function providerFields(settings: DictationSettings): string {
  const asr = ensure(settings, settings.asrProvider);
  const llm = ensure(settings, settings.llmProvider);
  const asrPreset = preset(settings.asrProvider);
  const llmPreset = preset(settings.llmProvider);
  const same = settings.asrProvider === settings.llmProvider;
  const suggestions = unique([...(asrPreset?.asrModels ?? []), ...(llmPreset?.llmModels ?? []), ...remoteModels]);
  const list = suggestions.map((id) => `<option value="${escapeHtml(id)}"></option>`).join("");
  const custom = settings.asrProvider === "custom" || settings.llmProvider === "custom";
  const customBlock = custom
    ? `<label class="field">Speech base URL<input id="asr-base" value="${escapeHtml(ensure(settings, "custom").asrBase || ensure(settings, "custom").base)}" placeholder="https://host/v1" /></label>
       <label class="field">Rewrite base URL<input id="llm-base" value="${escapeHtml(ensure(settings, "custom").llmBase || ensure(settings, "custom").base)}" placeholder="https://host/v1" /></label>
       <label class="field">Speech request<select id="asr-wire"><option value="transcriptions">/audio/transcriptions</option><option value="dashscope">DashScope audio chat</option></select></label>
       <label class="field">Custom thinking<select id="custom-thinking"><option value="auto">OpenAI none</option><option value="openai-low">OpenAI low</option><option value="dashscope">DashScope</option><option value="volcengine">Volcengine</option><option value="off">Temperature only</option></select></label>`
    : "";
  const asrBase = asrPreset?.baseEditable && settings.asrProvider !== "custom"
    ? `<label class="field">Speech base URL<input id="provider-base" value="${escapeHtml(asr.base)}" placeholder="https://{WorkspaceId}.cn-beijing.maas.aliyuncs.com/compatible-mode/v1" /></label>`
    : "";
  const llmBase = !same && llmPreset?.baseEditable && settings.llmProvider !== "custom"
    ? `<label class="field">Rewrite base URL<input id="llm-provider-base" value="${escapeHtml(llm.base)}" /></label>`
    : "";
  const volc = settings.asrProvider === "volcengine" || settings.llmProvider === "volcengine";
  const volcBlock = volc
    ? `<p class="status">${ui("New console: speech API key. Old console: App ID and access token. The rewrite key is the Ark key.", "新版控制台填语音 API Key。旧版填 App ID 和 Access Token。改写用方舟的 Key。")}</p>
       <label class="field">${ui("Volcengine App ID", "火山 App ID")}<input id="volc-app-id" value="${escapeHtml(ensure(settings, "volcengine").volcAppId)}" placeholder="123456789" /></label>
       <label class="field">${ui("Speech API key", "语音 API Key")}<input id="volc-speech-key" type="password" autocomplete="off" placeholder="${snap.keys["volcengine-speech"] ? ui("Saved. Enter a new key to replace it.", "已保存。输入新密钥以替换。") : ui("Not saved", "未保存")}" /></label>
       <label class="field">${ui("Access token", "Access Token")}<input id="volc-token" type="password" autocomplete="off" placeholder="${snap.keys["volcengine-token"] ? ui("Saved. Enter a new token to replace it.", "已保存。输入新 token 以替换。") : ui("Not saved", "未保存")}" /></label>`
    : "";
  const baseBlock = `${asrBase}${llmBase}${customBlock}${volcBlock}`;
  return `${baseBlock}
    <label class="field">${ui("Speech model", "语音模型")}<input id="asr-model" list="model-list" value="${escapeHtml(asr.asrModel)}" /></label>
    <label class="field">${ui("Rewrite model", "改写模型")}<input id="llm-model" list="model-list" value="${escapeHtml(same ? asr.llmModel : llm.llmModel)}" /></label>
    <datalist id="model-list">${list}</datalist>`;
}

function hotkeyRow(mode: HotkeyModeName, title: string, index: number): string {
  const editor = editors[mode];
  const activeChord = editor.activeChord || snap.hotkeyChords[index] || "";
  const activeLabel = editor.activeLabel || snap.hotkeyLabels[index] || "";
  const pendingWarning = editor.phase === "pending" ? knownConflict(editor.pendingChord) : null;
  const activeWarning = editor.phase === "pending" ? null : knownConflict(activeChord);
  const activeWarningLine = activeWarning ? `<p class="status warn">${escapeHtml(activeWarning)}</p>` : "";
  const pendingWarningLine = pendingWarning ? `<p class="status warn">${escapeHtml(pendingWarning)}</p>` : "";
  const choose = mode === "dictate" ? dictateKeySelect(activeChord) : "";
  let controls = `${choose}<div class="row"><button type="button" data-capture="${mode}">${ui("Capture", "捕获")}</button></div>`;
  if (editor.phase === "capturing") {
    const previewKeys = preview
      ? `<div class="row">${PREVIEW_CHORDS.map(
          ([chord, label]) =>
            `<button type="button" data-mode="${mode}" data-preview-chord="${escapeHtml(chord)}" data-preview-label="${escapeHtml(label)}">${escapeHtml(label)}</button>`,
        ).join("")}</div>`
      : "";
    const heard = editor.pendingLabel
      ? `<p class="status">${ui("Heard", "已按下")}: <strong>${escapeHtml(editor.pendingLabel)}</strong></p>`
      : "";
    controls = `<p class="status">${preview ? ui("Preview has no keyboard hook. Choose a chord.", "预览没有键盘钩子。请选一个组合。") : ui("Press the keys, then release them. Escape cancels.", "按下按键后松开。Esc 取消。")}</p>${heard}${previewKeys}<div class="row"><button type="button" data-cancel="${mode}">${ui("Cancel", "取消")}</button></div>`;
  } else if (editor.phase === "pending") {
    controls = `<p class="status">${ui("Captured", "已捕获")}：<strong>${escapeHtml(editor.pendingLabel)}</strong> <code>${escapeHtml(editor.pendingChord)}</code></p>${pendingWarningLine}<div class="row"><button type="button" class="primary" data-set="${mode}">${ui("Set", "设置")}</button><button type="button" data-capture="${mode}">${ui("Capture again", "重新捕获")}</button><button type="button" data-cancel="${mode}">${ui("Cancel", "取消")}</button></div>`;
  }
  return `<div class="hotkey-row" data-mode="${mode}"><h3>${escapeHtml(title)}</h3><p class="status">${ui("Active", "当前")}：<strong>${escapeHtml(activeLabel)}</strong> <code>${escapeHtml(activeChord)}</code></p>${activeWarningLine}${controls}</div>`;
}

function dictateKeySelect(activeChord: string): string {
  const selected = activeChord === "Vk20" ? "CapsLock" : activeChord;
  const options = DICTATE_KEYS.map(
    ([value, en, zh]) =>
      `<option value="${value}" ${selected === value ? "selected" : ""}>${escapeHtml(ui(en, zh))}</option>`,
  ).join("");
  return `<label class="field">${ui("Or choose a key", "或选择按键")}<select data-dictate-key><option value="">${ui("Choose…", "请选择…")}</option>${options}</select></label>`;
}

async function chooseHotkey(mode: HotkeyModeName, chord: string): Promise<void> {
  captureTicket += 1;
  if (!preview) {
    await call("dictation_cancel_capture").catch(() => undefined);
  }
  for (const [name] of HOTKEY_MODES) {
    if (editors[name].phase !== "idle") {
      editors[name] = cancelEditor(editors[name]);
    }
  }
  const chords = HOTKEY_MODES.map(([name], index) =>
    name === mode ? chord : editors[name].activeChord || snap.hotkeyChords[index] || "",
  );
  const localError = bindingError(chords);
  if (localError) {
    setHotkeyMessage(localError, true);
    renderHotkeys();
    return;
  }
  const found = HOTKEY_MODES.find(([name]) => name === mode);
  const title = found ? ui(found[1], found[2]) : mode;
  const choice = DICTATE_KEYS.find(([value]) => value === chord);
  const label = choice ? ui(choice[1], choice[2]) : chord;
  if (preview) {
    snap.settings.dictateHotkey = chords[0] ?? "";
    snap.settings.translateHotkey = chords[1] ?? "";
    snap.settings.commandHotkey = chords[2] ?? "";
    snap.hotkeyChords = chords;
    snap.hotkeyLabels = HOTKEY_MODES.map((_, index) => (index === 0 ? label : snap.hotkeyLabels[index] ?? ""));
    snap.warnings = bindingWarnings(chords);
    editors[mode] = idleEditor(chord, label);
    syncEditorsFromSnapshot();
    setHotkeyMessage(appliedMessage(title, label, snap.warnings));
    renderHotkeys();
    return;
  }
  try {
    const next = await call<Snapshot>("dictation_set_hotkey", { mode, hotkey: chord });
    const index = HOTKEY_MODES.findIndex(([name]) => name === mode);
    const applied = next.hotkeyLabels[index] ?? label;
    snap.hotkeyLabels = next.hotkeyLabels;
    snap.hotkeyChords = next.hotkeyChords;
    snap.warnings = next.warnings;
    snap.settings.dictateHotkey = next.settings.dictateHotkey;
    snap.settings.translateHotkey = next.settings.translateHotkey;
    snap.settings.commandHotkey = next.settings.commandHotkey;
    editors[mode] = idleEditor(next.hotkeyChords[index] ?? chord, applied);
    syncEditorsFromSnapshot();
    setHotkeyMessage(appliedMessage(title, applied, next.warnings));
    renderHotkeys();
  } catch (error) {
    setHotkeyMessage(String(error), true);
    renderHotkeys();
  }
}

function hotwordRow(word: Hotword, index: number): string {
  const haystack = `${word.canonical} ${word.aliases.join(" ")} ${word.pattern}`.toLowerCase();
  const hidden = dictionaryQuery.trim() && !haystack.includes(dictionaryQuery.trim().toLowerCase()) ? " hidden" : "";
  return `<div class="hotword${hidden}" data-index="${index}">
    <input class="canonical" placeholder="${ui("Term", "词条")}" value="${escapeHtml(word.canonical)}" />
    <input class="aliases" placeholder="${ui("Replacements, comma separated", "替换，逗号分隔")}" value="${escapeHtml(word.aliases.join(", "))}" />
    <input class="pattern" placeholder="${ui("Optional regex", "可选正则")}" value="${escapeHtml(word.pattern)}" />
    <button type="button" data-remove-term="${index}">${ui("Delete", "删除")}</button>
  </div>`;
}

function bindHotwordButtons(): void {
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-remove-term]")) {
    button.addEventListener("click", () => {
      button.closest(".hotword")?.remove();
      readHotwords();
      paint();
    });
  }
}

function historyRow(entry: HistoryEntry): string {
  const when = new Date(entry.createdAtMs).toLocaleString();
  const flag = entry.usedFallback ? ` · ${ui("ruled fallback", "规则回退")}` : "";
  const kept = entry.audioKept ? ` · ${ui("recording saved", "已保存录音")}` : "";
  const note = entry.note ? `<p class="muted">${escapeHtml(entry.note)}</p>` : "";
  const rerun = entry.audioKept
    ? `<button type="button" data-rerun="${escapeHtml(entry.id)}">${ui("Re-run", "重新识别")}</button>`
    : "";
  const hit = dictionaryHitLine(entry);
  const hitLine = hit ? `<p class="status">${escapeHtml(hit)}</p>` : "";
  return `<li><p><strong>${escapeHtml(entry.text || ui("(empty)", "（空）"))}</strong></p><p class="muted">${escapeHtml(when)} · ${escapeHtml(modeLabel(entry.mode))} · ${escapeHtml(entry.route)} · ${escapeHtml(entry.provider)} · ${escapeHtml(entry.appVersion)}${flag}${kept}</p>${hitLine}${note}<div class="row"><button type="button" data-copy="${escapeHtml(entry.id)}">${ui("Copy", "复制")}</button><button type="button" data-reinsert="${escapeHtml(entry.id)}">${ui("Insert again", "再次插入")}</button>${rerun}<button type="button" data-delete="${escapeHtml(entry.id)}">${ui("Delete", "删除")}</button></div></li>`;
}

function lastResultCard(): string {
  const entry = history[0];
  const title = ui("Last result", "上一条结果");
  if (!entry) {
    return `<section class="last-result" id="last-result"><h2>${title}</h2><p class="status">${ui("No history yet.", "还没有历史。")}</p></section>`;
  }
  const hit = dictionaryHitLine(entry);
  return `<section class="last-result" id="last-result"><h2>${title}</h2><p class="last-text">${escapeHtml(entry.text || ui("(empty)", "（空）"))}</p><p class="muted">${escapeHtml(modeLabel(entry.mode))} · ${escapeHtml(entry.route)} · ${escapeHtml(entry.provider)}</p>${hit ? `<p class="status">${escapeHtml(hit)}</p>` : ""}<div class="row"><button type="button" class="primary" data-copy="${escapeHtml(entry.id)}">${ui("Copy", "复制")}</button><button type="button" data-reinsert="${escapeHtml(entry.id)}">${ui("Insert again", "再次插入")}</button></div></section>`;
}

function dictionaryHitLine(entry: HistoryEntry): string {
  const applied = entry.dictionaryApplied ?? [];
  const possible = entry.dictionaryPossible ?? [];
  if (applied.length === 0 && possible.length === 0) {
    return "";
  }
  const en = [
    applied.length > 0 ? `applied ${applied.join(", ")}` : "",
    possible.length > 0 ? `possible ${possible.join(", ")}` : "",
  ]
    .filter(Boolean)
    .join(", ");
  const zh = [
    applied.length > 0 ? `已用 ${applied.join("、")}` : "",
    possible.length > 0 ? `可能 ${possible.join("、")}` : "",
  ]
    .filter(Boolean)
    .join("，");
  return ui(`Dictionary: ${en}`, `词典：${zh}`);
}

function onCaptureKey(event: KeyboardEvent): void {
  const capturing = HOTKEY_MODES.some(([mode]) => editors[mode].phase === "capturing");
  if (!capturing || event.repeat || preview) {
    return;
  }
  event.preventDefault();
  void call("dictation_capture_key", {
    code: event.code,
    down: event.type === "keydown",
    alt: event.altKey,
  });
}

function modeLabel(mode: string): string {
  switch (mode) {
    case "translate":
      return ui("Translate", "翻译");
    case "command":
      return ui("Command", "指令");
    default:
      return ui("Dictate", "口授");
  }
}

function visibleHistory(): HistoryEntry[] {
  const query = historyQuery.trim().toLowerCase();
  if (!query) {
    return history;
  }
  return history.filter((entry) =>
    `${entry.text} ${entry.raw} ${entry.mode} ${entry.route} ${entry.provider} ${entry.note}`.toLowerCase().includes(query),
  );
}

function retentionOptions(selected: string): string {
  const choices: Array<[string, string, string]> = [
    ["forever", "Forever", "永久"],
    ["year", "1 year", "1 年"],
    ["month", "1 month", "1 个月"],
    ["week", "1 week", "1 周"],
    ["day", "24 hours", "24 小时"],
    ["off", "Don't keep", "不保留"],
  ];
  return choices
    .map(
      ([value, en, zh]) =>
        `<option value="${value}" ${value === selected ? "selected" : ""}>${escapeHtml(ui(en, zh))}</option>`,
    )
    .join("");
}

function promptButtons(action: string): string {
  const mic = `<button type="button" id="prompt-mic">${ui("Open Microphone settings", "打开麦克风设置")}</button>`;
  const ax = `<button type="button" id="prompt-ax">${ui("Open Accessibility settings", "打开辅助功能设置")}</button>`;
  if (action === "microphone") {
    return mic;
  }
  if (action === "both") {
    return `${mic}${ax}`;
  }
  return ax;
}

function radios(name: string, choices: Array<[string, string, string]>, selected: string): string {
  return `<fieldset>${choices
    .map(
      ([value, title, detail]) =>
        `<label class="choice"><input type="radio" name="${name}" value="${value}" ${value === selected ? "checked" : ""} /><span>${escapeHtml(title)}<small>${escapeHtml(detail)}</small></span></label>`,
    )
    .join("")}</fieldset>`;
}

function options(presets: PresetInfo[], selected: string, ready: (preset: PresetInfo) => boolean): string {
  return presets
    .map((preset) => {
      const mark = ready(preset) ? "" : ui(" · not finished", " · 未完成");
      return `<option value="${escapeHtml(preset.id)}" ${preset.id === selected ? "selected" : ""}>${escapeHtml(preset.label)}${mark}</option>`;
    })
    .join("");
}

function badge(preset: PresetInfo | undefined): string {
  if (!preset) {
    return "";
  }
  if (!preset.asrReady || preset.note.startsWith("Partial")) {
    return `<span class="badge warn">Partial</span>`;
  }
  return `<span class="badge">Ready</span>`;
}

function activeBundle(settings: DictationSettings): BundleInfo | undefined {
  return snap.bundles.find((bundle) => {
    const asr = settings.providers[bundle.asrProvider];
    const llm = settings.providers[bundle.llmProvider];
    return (
      settings.asrProvider === bundle.asrProvider &&
      settings.llmProvider === bundle.llmProvider &&
      asr?.asrModel === bundle.asrModel &&
      llm?.llmModel === bundle.llmModel
    );
  });
}

function applyBundle(id: string): void {
  const bundle = snap.bundles.find((item) => item.id === id);
  if (!bundle) {
    return;
  }
  snap.settings.asrProvider = bundle.asrProvider;
  snap.settings.llmProvider = bundle.llmProvider;
  ensure(snap.settings, bundle.asrProvider).asrModel = bundle.asrModel;
  ensure(snap.settings, bundle.llmProvider).llmModel = bundle.llmModel;
  paint();
  void persist({
    asrProvider: snap.settings.asrProvider,
    llmProvider: snap.settings.llmProvider,
    providers: snap.settings.providers,
  });
}

function groqFallbackControl(settings: DictationSettings): string {
  if (settings.asrProvider !== "groq" && settings.llmProvider !== "groq") {
    return "";
  }
  const checked = settings.groqRateLimitFallback !== false ? "checked" : "";
  return `<label class="choice"><input type="checkbox" id="groq-openai-fallback" ${checked} /><span>${ui("On Groq rate limit, fall back to OpenAI", "Groq 触发限流时改用 OpenAI")}<small>${ui("Uses gpt-transcribe for speech and gpt-4o-mini for rewrite. Requires an OpenAI key. The next dictation tries Groq again.", "语音用 gpt-transcribe，改写用 gpt-4o-mini。需要 OpenAI 密钥。下一次仍先试 Groq。")}</small></span></label>`;
}

function presetCostLine(settings: DictationSettings): string {
  const bundle = activeBundle(settings);
  if (!bundle) {
    return ui(
      "Pick Groq (~$1.8/mo), Groq + GPT-4o mini (~$2.5/mo), or OpenAI (~$8/mo). Estimates are for about 200 dictations a day.",
      "可选 Groq（约 $1.8/月）、Groq + GPT-4o mini（约 $2.5/月）或 OpenAI（约 $8/月）。按每天约 200 次估算。",
    );
  }
  return `${escapeHtml(bundle.note)} ${ui(
    `About $${bundle.monthlyUsd}/month at 200 dictations a day.`,
    `每天约 200 次时大约 $${bundle.monthlyUsd}/月。`,
  )}`;
}

function keyStatus(settings: DictationSettings): string {
  const missing: string[] = [];
  const speechName = preset(settings.asrProvider)?.label ?? settings.asrProvider;
  if (!snap.keys[settings.asrProvider]) {
    missing.push(ui(`${speechName} speech key`, `${speechName} 语音密钥`));
  }
  if (settings.llmProvider !== settings.asrProvider && !snap.keys[settings.llmProvider]) {
    const rewriteName = preset(settings.llmProvider)?.label ?? settings.llmProvider;
    missing.push(ui(`${rewriteName} rewrite key`, `${rewriteName} 改写密钥`));
  }
  if (missing.length === 0) {
    return `<p class="status">${ui("The keys for this preset are saved.", "这个方案需要的密钥都已保存。")}</p>`;
  }
  return `<p class="status warn">${ui("Missing", "缺少")}: ${escapeHtml(missing.join(", "))}. ${ui("Speech and rewrite keys are stored separately.", "语音密钥和改写密钥分开保存。")}</p>`;
}

function preset(id: string): PresetInfo | undefined {
  return snap.presets.find((item) => item.id === id);
}

function ensure(settings: DictationSettings, id: string): ProviderConfig {
  const found = settings.providers[id];
  if (found) {
    return found;
  }
  const created: ProviderConfig = {
    asrModel: "",
    llmModel: "",
    base: "",
    asrBase: "",
    llmBase: "",
    asrWire: "transcriptions",
    thinking: "auto",
    volcAppId: "",
  };
  settings.providers[id] = created;
  return created;
}

function unique(values: string[]): string[] {
  return [...new Set(values.filter(Boolean))];
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

function wireUpdates(): void {
  const check = document.querySelector<HTMLButtonElement>("#check");
  const apply = document.querySelector<HTMLButtonElement>("#apply");
  check?.addEventListener("click", () => {
    if (preview) {
      return;
    }
    void call<UpdateState>("check_for_updates").then(renderUpdate);
  });
  apply?.addEventListener("click", () => {
    if (preview) {
      return;
    }
    void call<UpdateState>("apply_update").then(renderUpdate);
  });
  for (const input of document.querySelectorAll<HTMLInputElement>('input[name="channel"]')) {
    input.addEventListener("change", () => {
      if (!input.checked || preview) {
        return;
      }
      void call<UpdateState>("set_channel", { channel: input.value }).then(renderUpdate);
    });
  }
}

function renderUpdate(state: UpdateState): void {
  const versionLine = document.querySelector("#version-line");
  const endpointLine = document.querySelector("#endpoint-line");
  const statusLine = document.querySelector("#status");
  const progress = document.querySelector<HTMLProgressElement>("#progress");
  const checkButton = document.querySelector<HTMLButtonElement>("#check");
  const applyButton = document.querySelector<HTMLButtonElement>("#apply");
  if (!versionLine || !endpointLine || !statusLine || !progress || !checkButton || !applyButton) {
    return;
  }
  versionLine.innerHTML = `<strong>${escapeHtml(state.fullVersion)}</strong><span>version ${escapeHtml(state.version)}</span><span>commit ${escapeHtml(state.commit)}</span>`;
  endpointLine.textContent = state.endpoint;
  for (const input of document.querySelectorAll<HTMLInputElement>('input[name="channel"]')) {
    input.checked = input.value === state.channel;
    input.disabled = state.phase === "checking" || state.phase === "downloading";
  }
  checkButton.textContent = ui("Check for updates", "检查更新");
  checkButton.disabled = state.phase === "checking" || state.phase === "downloading";
  statusLine.textContent = state.message;
  const showApply = state.phase === "ready" && state.availableVersion !== null;
  applyButton.hidden = !showApply;
  if (showApply && state.availableVersion) {
    applyButton.textContent = `${ui("Restart to apply", "重启以应用")} ${state.availableVersion}`;
  }
  if (state.phase === "downloading" && state.totalBytes && state.receivedBytes !== null) {
    progress.hidden = false;
    progress.max = state.totalBytes;
    progress.value = state.receivedBytes;
  } else {
    progress.hidden = true;
  }
}
