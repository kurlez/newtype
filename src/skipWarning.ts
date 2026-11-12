/** Skip is recorded only after the warning is acknowledged. An empty warning does not skip. */
export function confirmSkip(message: string, ask: (text: string) => boolean): boolean {
  const text = message.trim();
  if (!text) {
    return false;
  }
  return ask(text);
}
