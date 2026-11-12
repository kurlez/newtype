/** Settings chrome follows the system language. Chinese and English are the v1 pair. */
export function ui(en: string, zh: string): string {
  if (typeof navigator !== "undefined" && navigator.language.toLowerCase().startsWith("zh")) {
    return zh;
  }
  return en;
}
