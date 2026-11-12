/** One locale at a time. `auto` follows the resolved system language from the app. */

export type UiLanguage = "en" | "zh";

const UI = {
  history: ["History", "历史"],
  copy: ["Copy", "复制"],
  insert: ["Insert again", "再次插入"],
  openHistory: ["Open in History", "在历史中打开"],
  cancel: ["Cancel", "取消"],
  recording: ["Recording", "录音中"],
  processing: ["Processing", "处理中"],
  inserted: ["Inserted", "已插入"],
  error: ["Error", "出错"],
  silence: ["No speech", "无语音"],
  stub: ["Not in this build", "此构建没有"],
  cancelled: ["Cancelled", "已取消"],
  withheld: ["Not inserted", "未插入"],
  copied: ["Copied", "已复制"],
  idle: ["Idle", "空闲"],
  drag: ["Drag", "拖动"],
} as const;

export type UiKey = keyof typeof UI;

let current: UiLanguage = detect();

function detect(): UiLanguage {
  if (typeof navigator !== "undefined" && navigator.language.toLowerCase().startsWith("zh")) {
    return "zh";
  }
  return "en";
}

export function applyLanguage(setting: string, resolved: string): void {
  if (setting === "zh" || setting === "en") {
    current = setting;
    return;
  }
  current = resolved === "zh" ? "zh" : resolved === "en" ? "en" : detect();
}

export function ui(en: string, zh: string): string {
  return current === "zh" ? zh : en;
}

export function t(key: UiKey): string {
  const [en, zh] = UI[key];
  return ui(en, zh);
}

/** True when one string repeats the same phrase in Han and in Latin. */
export function mixesLanguages(value: string): boolean {
  const han = Array.from(value).some((ch) => ch >= "\u4e00" && ch <= "\u9fff");
  if (!han) {
    return false;
  }
  const words = value.toLowerCase().match(/[a-z]{3,}/g) ?? [];
  const allow = new Set([
    "api", "gpt", "json", "url", "qwen", "llama", "cpp", "openai", "groq", "macos", "windows",
    "ctrl", "alt", "app", "id", "key", "mica", "acrylic",
  ]);
  return words.some((word) => !allow.has(word));
}

export function uiTable(): Record<string, readonly [string, string]> {
  return UI;
}
