export interface ProviderConfig {
  asrModel: string;
  llmModel: string;
  base: string;
  asrBase: string;
  llmBase: string;
  asrWire: string;
  thinking: string;
}

export interface Hotword {
  canonical: string;
  aliases: string[];
  pattern: string;
}

export interface DictationSettings {
  asrProvider: string;
  llmProvider: string;
  talkMode: string;
  polishLevel: string;
  preference: string;
  dictateHotkey: string;
  translateHotkey: string;
  commandHotkey: string;
  thinking: string;
  microphone: string;
  providers: Record<string, ProviderConfig>;
  hotwords: Hotword[];
}

export interface PresetInfo {
  id: string;
  label: string;
  asrReady: boolean;
  llmReady: boolean;
  baseEditable: boolean;
  defaultBase: string;
  defaultAsr: string;
  defaultLlm: string;
  asrModels: string[];
  llmModels: string[];
  asrWire: string;
  note: string;
  verified: string;
}

export interface PermissionInfo {
  microphone: string;
  accessibility: string;
  platform: string;
  guidance: string;
}

export interface Snapshot {
  settings: DictationSettings;
  hotkeyLabels: string[];
  hotkeyChords: string[];
  warnings: string[];
  verifiedOn: string;
  keys: Record<string, boolean>;
  presets: PresetInfo[];
  permissions: PermissionInfo;
  listener: string;
  routeLabel: string;
  routeDetail: string;
  phase: string;
  version: string;
  devKeyStore: boolean;
}

export interface HistoryEntry {
  id: string;
  createdAtMs: number;
  appVersion: string;
  mode: string;
  route: string;
  provider: string;
  polishLevel: string;
  raw: string;
  ruled: string;
  text: string;
  usedFallback: boolean;
  note: string;
}

export interface PhaseEvent {
  phase: string;
  route: string;
  message: string;
  elapsedMs: number;
  level: number;
  text: string;
}

export interface CaptureEvent {
  mode: string;
  hotkey: string;
  label: string;
}

export interface UpdateState {
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

export function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

export function isPreview(): boolean {
  return new URLSearchParams(location.search).get("preview") === "1";
}
