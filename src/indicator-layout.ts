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
