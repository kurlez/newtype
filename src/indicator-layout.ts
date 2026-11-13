/** Visible status, one language. The mode and the phase stay separate from the route badge. */
export function statusLine(mode: string, phaseLabel: string): string {
  const left = mode.trim();
  const right = phaseLabel.trim();
  if (left && right) {
    return `${left} · ${right}`;
  }
  return left || right;
}

export interface RouteWords {
  auto: string;
  offline: string;
  local: string;
  cloud: string;
}

export function routeKind(route: string): "fallback" | "strict" | "local" | "cloud" {
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

export interface StripVars {
  fontPx: number;
  iconPx: number;
  controlPx: number;
  dotPx: number;
  gripPx: number;
}

/** Sizes for a taskbar strip whose CSS height is `heightPx`. 48px is the 100% Windows 11 bar. */
export function taskbarStripVars(heightPx: number): StripVars {
  const height = Number.isFinite(heightPx) && heightPx > 0 ? heightPx : 48;
  const scale = height / 48;
  const controlPx = clamp(Math.round(20 * scale), 14, Math.max(14, height - 4));
  const iconPx = clamp(Math.round(14 * scale), 10, Math.max(10, controlPx - 2));
  const fontPx = clamp(Math.round(12 * scale), 10, Math.max(10, height - 8));
  const dotPx = clamp(Math.round(8 * scale), 5, Math.max(5, controlPx - 4));
  const gripPx = clamp(Math.round(16 * scale), 8, Math.max(8, height - 8));
  return { fontPx, iconPx, controlPx, dotPx, gripPx };
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

/** Short chip. The full route string stays on the tooltip. */
export function routeBadge(route: string, words: RouteWords): string {
  switch (routeKind(route)) {
    case "fallback":
      return words.auto;
    case "strict":
      return words.offline;
    case "local":
      return words.local;
    default:
      return words.cloud;
  }
}
