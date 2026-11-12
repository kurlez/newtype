import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./styles.css";

interface UpdateState {
  version: string;
  commit: string;
  fullVersion: string;
  channel: string;
  endpoint: string;
  phase: string;
  message: string;
  availableVersion: string | null;
  receivedBytes: number | null;
  totalBytes: number | null;
}

interface PipelineModule {
  name: string;
  status: string;
  note: string;
}

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) {
  throw new Error("missing #app");
}

app.innerHTML = `
  <header>
    <h1>newtype</h1>
    <p>Menu bar shell. Voice input is not in this build.</p>
  </header>
  <section aria-labelledby="about-heading">
    <h2 id="about-heading">About</h2>
    <p class="version" id="version-line">Loading…</p>
    <p class="endpoint" id="endpoint-line"></p>
  </section>
  <section aria-labelledby="updates-heading">
    <h2 id="updates-heading">Updates</h2>
    <fieldset>
      <legend>Channel</legend>
      <label class="choice">
        <input type="radio" name="channel" value="dev" />
        <span>dev<small>Every push to main. This is the channel to stay on while testing.</small></span>
      </label>
      <label class="choice">
        <input type="radio" name="channel" value="stable" />
        <span>stable<small>Only <code>v*</code> tags. Nothing is published here until the first tag.</small></span>
      </label>
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
    <p class="lang">macOS · Gatekeeper</p>
    <ol>
      <li>Open the .dmg and drag newtype into Applications.</li>
      <li>Do not double-click the first time. The app is not notarized.</li>
      <li>In Finder, right-click newtype and choose Open, then Open again.</li>
      <li>If a double-click was already blocked: System Settings → Privacy &amp; Security → Open Anyway.</li>
    </ol>
    <p class="lang">macOS · 首次打开</p>
    <ol>
      <li>打开 .dmg，把 newtype 拖到「应用程序」。</li>
      <li>第一次不要双击。此应用没有 Apple 公证。</li>
      <li>在 Finder 中右键 newtype → 打开，再在对话框里点「打开」。</li>
      <li>如果已经双击被拦截：系统设置 → 隐私与安全性 → 仍要打开。</li>
    </ol>
    <p class="lang">Windows · SmartScreen</p>
    <ol>
      <li>Run the NSIS installer. It installs for the current user.</li>
      <li>If SmartScreen appears, choose More info, then Run anyway.</li>
    </ol>
    <p class="status">
      macOS builds are ad-hoc signed, or signed with a stable self-signed identity when that secret is configured.
      They are not Developer ID signed. Whether Accessibility and Microphone grants survive an update still has to be checked on a real Mac.
      This build does not request those permissions.
    </p>
  </section>
  <section aria-labelledby="later-heading">
    <h2 id="later-heading">Not in this build</h2>
    <ul class="stubs" id="stubs"></ul>
  </section>
`;

const versionLine = required("#version-line");
const endpointLine = required("#endpoint-line");
const statusLine = required("#status");
const progress = required<HTMLProgressElement>("#progress");
const checkButton = required<HTMLButtonElement>("#check");
const applyButton = required<HTMLButtonElement>("#apply");
const stubs = required("#stubs");
const channelInputs = Array.from(
  document.querySelectorAll<HTMLInputElement>('input[name="channel"]'),
);

function required<T extends Element = HTMLElement>(selector: string): T {
  const node = document.querySelector<T>(selector);
  if (!node) {
    throw new Error(`missing ${selector}`);
  }
  return node;
}

function render(state: UpdateState): void {
  versionLine.innerHTML = `<strong>${escapeHtml(state.fullVersion)}</strong><span>version ${escapeHtml(state.version)}</span><span>commit ${escapeHtml(state.commit)}</span>`;
  endpointLine.textContent = state.endpoint;
  for (const input of channelInputs) {
    input.checked = input.value === state.channel;
    input.disabled = state.phase === "checking" || state.phase === "downloading";
  }
  checkButton.disabled = state.phase === "checking" || state.phase === "downloading";
  statusLine.textContent = state.message;
  statusLine.className = `status ${state.phase === "error" ? "error" : state.phase === "ready" ? "ready" : ""}`;
  const showApply = state.phase === "ready" && state.availableVersion !== null;
  applyButton.hidden = !showApply;
  applyButton.disabled = !showApply;
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

checkButton.addEventListener("click", () => {
  void run(() => invoke<UpdateState>("check_for_updates"));
});

applyButton.addEventListener("click", () => {
  void run(() => invoke<UpdateState>("apply_update"));
});

for (const input of channelInputs) {
  input.addEventListener("change", () => {
    if (!input.checked) {
      return;
    }
    void run(() => invoke<UpdateState>("set_channel", { channel: input.value }));
  });
}

void listen<UpdateState>("update://state", (event) => {
  render(event.payload);
});

void (async () => {
  try {
    const [state, modules] = await Promise.all([
      invoke<UpdateState>("get_state"),
      invoke<PipelineModule[]>("pipeline_status"),
    ]);
    render(state);
    stubs.replaceChildren(
      ...modules.map((module) => {
        const item = document.createElement("li");
        item.innerHTML = `<strong>${escapeHtml(module.name)}</strong> <span>${escapeHtml(module.note)}</span>`;
        return item;
      }),
    );
  } catch (error) {
    statusLine.textContent = errorText(error);
    statusLine.className = "status error";
  }
})();

async function run(action: () => Promise<UpdateState>): Promise<void> {
  try {
    render(await action());
  } catch (error) {
    statusLine.textContent = errorText(error);
    statusLine.className = "status error";
  }
}

function errorText(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}
