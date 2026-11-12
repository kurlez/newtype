import { previewHistory, previewSnapshot } from "./preview";
import {
  escapeHtml,
  type CaptureEvent,
  type DictationSettings,
  type HistoryEntry,
  type Hotword,
  type PresetInfo,
  type ProviderConfig,
  type Snapshot,
  type UpdateState,
} from "./types";

const root = document.querySelector<HTMLDivElement>("#app");
let snap: Snapshot = previewSnapshot;
let history: HistoryEntry[] = [];
let notice = "";
let preview = false;
let remoteModels: string[] = [];

export async function mount(previewMode: boolean): Promise<void> {
  preview = previewMode;
  if (!root) {
    return;
  }
  root.innerHTML = `
    <header>
      <h1>newtype</h1>
      <p id="tagline">Cloud dictation for the focused app. Local models are not in this build.</p>
    </header>
    <div id="dictate"></div>
    <section aria-labelledby="updates-heading">
      <h2 id="updates-heading">Updates</h2>
      <p class="version" id="version-line">Loading…</p>
      <p class="endpoint" id="endpoint-line"></p>
      <fieldset>
        <legend>Channel</legend>
        <label class="choice"><input type="radio" name="channel" value="dev" /><span>dev<small>Every push to main.</small></span></label>
        <label class="choice"><input type="radio" name="channel" value="stable" /><span>stable<small>Only v* tags.</small></span></label>
      </fieldset>
      <div class="row">
        <button type="button" id="check" class="primary">Check for updates</button>
        <button type="button" id="apply" hidden>Restart to apply</button>
      </div>
      <p class="status" id="status" aria-live="polite"></p>
      <progress id="progress" hidden max="1" value="0"></progress>
    </section>
    <section aria-labelledby="install-heading">
      <h2 id="install-heading">First install</h2>
      <ol>
        <li>macOS: drag newtype to Applications. Do not double-click the first time. Right-click → Open, then Open. If a double-click was blocked: System Settings → Privacy &amp; Security → Open Anyway.</li>
        <li>macOS：第一次不要双击。右键 → 打开。若已拦截：系统设置 → 隐私与安全性 → 仍要打开。</li>
        <li>Windows: if SmartScreen appears, choose More info, then Run anyway.</li>
      </ol>
      <p class="status">Updates are not Developer ID signed. Microphone and Accessibility may need to be granted again after an update. That has to be checked on a real Mac.</p>
    </section>
    <section>
      <h2>About</h2>
      <p class="status">Cleanup prompts keep the MIT attribution for ckckck/OpenTypeless. No prompt text is taken from Open-Less/openless. Qwen and Doubao weights are not bundled in this build.</p>
    </section>
  `;
  wireUpdates();
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
  const { listen } = await import("@tauri-apps/api/event");
  await listen<UpdateState>("update://state", (event) => renderUpdate(event.payload));
  await listen<CaptureEvent>("dictation://capture", (event) => {
    const input = document.querySelector<HTMLInputElement>(`#hotkey-${event.payload.mode}`);
    if (input) {
      input.value = event.payload.hotkey;
      notice = `${event.payload.label} captured. Save to keep it.`;
      const note = document.querySelector("#notice");
      if (note) {
        note.textContent = notice;
      }
    }
  });
  const { invoke } = await import("@tauri-apps/api/core");
  renderUpdate(await invoke<UpdateState>("get_state"));
  await reload();
}

async function reload(): Promise<void> {
  snap = await call<Snapshot>("dictation_snapshot");
  history = await call<HistoryEntry[]>("dictation_history");
  paint();
}

function paint(): void {
  const host = document.querySelector("#dictate");
  if (!host) {
    return;
  }
  const settings = snap.settings;
  const asr = preset(settings.asrProvider);
  const llm = preset(settings.llmProvider);
  host.innerHTML = `
    <section>
      <h2>Path</h2>
      <p class="banner">${escapeHtml(snap.routeLabel)}</p>
      <p class="status">${escapeHtml(snap.routeDetail)}</p>
      <p class="status">${escapeHtml(snap.listener)}</p>
      ${radios("preference", [
        ["cloudDefault", "Cloud default", "Use the configured provider. Local fallback is decided by the router and is not linked yet."],
        ["localOnly", "Local only", "Not available in this build. Audio will not be sent."],
        ["strictOffline", "Strict offline", "Not available in this build. Nothing is uploaded."],
      ], settings.preference)}
    </section>
    <section>
      <h2>Permissions</h2>
      <p class="status">${escapeHtml(snap.permissions.guidance)}</p>
      <p class="status">Microphone: ${escapeHtml(snap.permissions.microphone)} · Accessibility: ${escapeHtml(snap.permissions.accessibility)}</p>
      <div class="row">
        <button type="button" id="perm-mic" class="primary">Allow microphone</button>
        <button type="button" id="perm-ax">Allow Accessibility</button>
        <button type="button" id="open-mic">Microphone settings</button>
        <button type="button" id="open-ax">Accessibility settings</button>
        <button type="button" id="open-input">Input Monitoring</button>
      </div>
    </section>
    <section>
      <h2>Provider</h2>
      <p class="status">Model ids were checked ${escapeHtml(snap.verifiedOn)}. The fields stay editable. Load models asks the provider what this key can see.</p>
      ${snap.devKeyStore ? `<p class="status warn">This machine is not macOS or Windows. The key is kept in a local dev file, not the OS keychain.</p>` : `<p class="status">API keys are stored in the macOS Keychain or Windows Credential Manager. They are not written to settings.</p>`}
      <label class="field">Speech<select id="asr-provider">${options(snap.presets, settings.asrProvider, (item) => item.asrReady)}</select></label>
      <p class="status">${badge(asr)} ${escapeHtml(asr?.note ?? "")}</p>
      <label class="field">Rewrite<select id="llm-provider">${options(snap.presets, settings.llmProvider, (item) => item.llmReady)}</select></label>
      <p class="status">${badge(llm)} ${escapeHtml(llm?.note ?? "")}</p>
      ${providerFields(settings)}
      <label class="field">Thinking
        <select id="thinking">
          <option value="auto" ${settings.thinking === "auto" ? "selected" : ""}>Auto (off where the model allows it)</option>
          <option value="off" ${settings.thinking === "off" ? "selected" : ""}>Force off</option>
          <option value="low" ${settings.thinking === "low" ? "selected" : ""}>Low effort when off is rejected</option>
        </select>
      </label>
      <label class="field">API key for ${escapeHtml(settings.asrProvider)}
        <input id="api-key" type="password" autocomplete="off" placeholder="${snap.keys[settings.asrProvider] ? "Saved. Enter a new key to replace it." : "Not saved"}" />
      </label>
      ${settings.llmProvider !== settings.asrProvider ? `<label class="field">API key for ${escapeHtml(settings.llmProvider)}<input id="llm-key" type="password" autocomplete="off" placeholder="${snap.keys[settings.llmProvider] ? "Saved. Enter a new key to replace it." : "Not saved"}" /></label>` : ""}
      <div class="row">
        <button type="button" id="save-key" class="primary">Save key</button>
        <button type="button" id="clear-key">Clear speech key</button>
        <button type="button" id="test-key">Test connection</button>
        <button type="button" id="load-models">Load models</button>
      </div>
    </section>
    <section>
      <h2>Hotkey</h2>
      <p class="status">Both desktops default to Right Ctrl, the right-hand Control key (ControlRight), not Left Control and not Right Option. Right Option is Typeless on a PC keyboard, including on a Mac. Escape cancels.</p>
      ${radios("talk", [["toggle", "Toggle", "Press to start, press again to stop."], ["hold", "Hold to talk", "Release to stop."]], settings.talkMode)}
      ${hotkeyRow("dictate", "Dictate", 0)}
      ${hotkeyRow("translate", "Translate (stub)", 1)}
      ${hotkeyRow("command", "Command (stub)", 2)}
      <div class="row"><button type="button" id="defaults">Restore default hotkeys</button></div>
      <ul class="notes">${snap.warnings.map((warning) => `<li>${escapeHtml(warning)}</li>`).join("")}</ul>
    </section>
    <section>
      <h2>Polish</h2>
      ${radios("polish", [["polish", "Polish", "Smoother wording. Does not answer or carry out the request."], ["literal", "Literal", "Minimal cleanup. Wording stays close to the transcript."]], settings.polishLevel)}
    </section>
    <section>
      <h2>Hot words</h2>
      <p class="status">Aliases and patterns run before the model. The same list is shown to the model. A spelling with no alias is prompt-only.</p>
      <div id="hotwords">${settings.hotwords.map((word, index) => hotwordRow(word, index)).join("")}</div>
      <button type="button" id="add-hotword">Add hot word</button>
    </section>
    <section>
      <h2>Microphone</h2>
      <label class="field">Device<input id="microphone" list="mic-list" value="${escapeHtml(settings.microphone)}" placeholder="System default" /></label>
      <datalist id="mic-list"></datalist>
      <div class="row"><button type="button" id="refresh-mics">List devices</button><button type="button" id="probe-mic">Test microphone</button><button type="button" id="record">Record a test</button></div>
    </section>
    <section>
      <h2>History</h2>
      <p class="status">On this device only. Each entry stores the app version. The latest 300 are kept.</p>
      <ul class="history">${history.map((entry) => historyRow(entry)).join("") || "<li>No dictations yet.</li>"}</ul>
    </section>
    <section>
      <div class="row"><button type="button" id="save" class="primary">Save settings</button><button type="button" id="reset-models">Reset suggested models</button></div>
      <p class="status" id="notice">${escapeHtml(notice)}</p>
    </section>
  `;
  bind();
}

function bind(): void {
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
      if (input.checked) {
        snap.settings.preference = input.value;
      }
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
  document.querySelector("#save-key")?.addEventListener("click", () => void saveKeys());
  document.querySelector("#clear-key")?.addEventListener("click", () => void clearKey(snap.settings.asrProvider));
  document.querySelector("#test-key")?.addEventListener("click", () => void testKey(snap.settings.asrProvider));
  document.querySelector("#load-models")?.addEventListener("click", () => void loadModels(snap.settings.asrProvider));
  document.querySelector("#defaults")?.addEventListener("click", () => {
    snap.settings.dictateHotkey = "";
    snap.settings.translateHotkey = "";
    snap.settings.commandHotkey = "";
    void save();
  });
  document.querySelector("#reset-models")?.addEventListener("click", () => void resetModels());
  document.querySelector("#add-hotword")?.addEventListener("click", () => {
    readHotwords();
    snap.settings.hotwords.push({ canonical: "", aliases: [], pattern: "" });
    paint();
  });
  document.querySelector("#perm-mic")?.addEventListener("click", () => void permission("microphone"));
  document.querySelector("#perm-ax")?.addEventListener("click", () => void permission("accessibility"));
  document.querySelector("#open-mic")?.addEventListener("click", () => void openPrivacy("microphone"));
  document.querySelector("#open-ax")?.addEventListener("click", () => void openPrivacy("accessibility"));
  document.querySelector("#open-input")?.addEventListener("click", () => void openPrivacy("input"));
  document.querySelector("#refresh-mics")?.addEventListener("click", () => void listMics());
  document.querySelector("#probe-mic")?.addEventListener("click", () => void probe());
  document.querySelector("#record")?.addEventListener("click", () => void record());
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-capture]")) {
    button.addEventListener("click", () => void capture(button.dataset.capture ?? "dictate"));
  }
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-copy]")) {
    button.addEventListener("click", () => void copyHistory(button.dataset.copy ?? ""));
  }
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-delete]")) {
    button.addEventListener("click", () => void deleteHistory(button.dataset.delete ?? ""));
  }
}

function readForm(): void {
  const settings = snap.settings;
  const read = (id: string) => document.querySelector<HTMLInputElement>(id)?.value ?? "";
  settings.dictateHotkey = read("#hotkey-dictate");
  settings.translateHotkey = read("#hotkey-translate");
  settings.commandHotkey = read("#hotkey-command");
  settings.microphone = read("#microphone");
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

async function save(): Promise<void> {
  readForm();
  if (preview) {
    notice = "Preview only. Settings are not saved.";
    paint();
    return;
  }
  try {
    snap = await call<Snapshot>("dictation_save", { settings: snap.settings });
    notice = "Saved.";
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
  if (preview) {
    if (speech.trim()) {
      snap.keys[snap.settings.asrProvider] = true;
    }
    notice = "Preview only. The key was not stored.";
    paint();
    return;
  }
  try {
    if (speech.trim()) {
      await call("dictation_set_key", { provider: snap.settings.asrProvider, key: speech });
    }
    if (rewrite.trim()) {
      await call("dictation_set_key", { provider: snap.settings.llmProvider, key: rewrite });
    }
    notice = "Key saved in the OS keychain.";
    await reload();
    notice = "Key saved in the OS keychain.";
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

async function testKey(provider: string): Promise<void> {
  if (preview) {
    notice = "Preview cannot reach a provider.";
    paint();
    return;
  }
  try {
    notice = await call<string>("dictation_test_connection", { provider });
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
    remoteModels = ["gpt-transcribe", "gpt-6-luna", "gpt-6.1-sol", "gpt-6-astra"];
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

async function capture(mode: string): Promise<void> {
  if (preview) {
    const input = document.querySelector<HTMLInputElement>(`#hotkey-${mode}`);
    if (input) {
      input.value = "ControlRight";
    }
    notice = "Preview captured Right Ctrl. Save to keep it.";
    const node = document.querySelector("#notice");
    if (node) {
      node.textContent = notice;
    }
    return;
  }
  notice = "Press the keys, then release them.";
  const node = document.querySelector("#notice");
  if (node) {
    node.textContent = notice;
  }
  await call("dictation_capture_hotkey", { mode });
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
  const baseBlock = `${asrBase}${llmBase}${customBlock}`;
  return `${baseBlock}
    <label class="field">Speech model<input id="asr-model" list="model-list" value="${escapeHtml(asr.asrModel)}" /></label>
    <label class="field">Rewrite model<input id="llm-model" list="model-list" value="${escapeHtml(same ? asr.llmModel : llm.llmModel)}" /></label>
    <datalist id="model-list">${list}</datalist>`;
}

function hotkeyRow(mode: string, title: string, index: number): string {
  const value = snap.hotkeyChords[index] ?? "";
  const label = snap.hotkeyLabels[index] ?? "";
  return `<label class="field">${escapeHtml(title)} <span class="muted">${escapeHtml(label)}</span><span class="hotkey-line"><input id="hotkey-${mode}" value="${escapeHtml(value)}" /><button type="button" data-capture="${mode}">Set</button></span></label>`;
}

function hotwordRow(word: Hotword, index: number): string {
  return `<div class="hotword" data-index="${index}">
    <input class="canonical" placeholder="Exact spelling" value="${escapeHtml(word.canonical)}" />
    <input class="aliases" placeholder="Mishearings, comma separated" value="${escapeHtml(word.aliases.join(", "))}" />
    <input class="pattern" placeholder="Optional regex" value="${escapeHtml(word.pattern)}" />
  </div>`;
}

function historyRow(entry: HistoryEntry): string {
  const when = new Date(entry.createdAtMs).toLocaleString();
  const flag = entry.usedFallback ? " · ruled fallback" : "";
  return `<li><p><strong>${escapeHtml(entry.text || "(empty)")}</strong></p><p class="muted">${escapeHtml(when)} · ${escapeHtml(entry.appVersion)} · ${escapeHtml(entry.route)}${flag}</p><div class="row"><button type="button" data-copy="${escapeHtml(entry.id)}">Copy</button><button type="button" data-delete="${escapeHtml(entry.id)}">Delete</button></div></li>`;
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
      const mark = ready(preset) ? "" : " · not finished";
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
  checkButton.disabled = state.phase === "checking" || state.phase === "downloading";
  statusLine.textContent = state.message;
  const showApply = state.phase === "ready" && state.availableVersion !== null;
  applyButton.hidden = !showApply;
  if (showApply && state.availableVersion) {
    applyButton.textContent = `Restart to apply ${state.availableVersion}`;
  }
  if (state.phase === "downloading" && state.totalBytes && state.receivedBytes !== null) {
    progress.hidden = false;
    progress.max = state.totalBytes;
    progress.value = state.receivedBytes;
  } else {
    progress.hidden = true;
  }
}
