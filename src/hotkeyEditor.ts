export type HotkeyModeName = "dictate" | "translate" | "command";
export type HotkeyPhase = "idle" | "capturing" | "pending";

export interface HotkeyEditor {
  phase: HotkeyPhase;
  activeChord: string;
  activeLabel: string;
  pendingChord: string;
  pendingLabel: string;
}

const MODE_NAMES = ["Dictate", "Translate", "Command"];

export function idleEditor(activeChord: string, activeLabel: string): HotkeyEditor {
  return {
    phase: "idle",
    activeChord,
    activeLabel,
    pendingChord: "",
    pendingLabel: "",
  };
}

export const CAPTURE_SILENCE_MS = 3000;

export function captureHeardNothing(phase: HotkeyPhase, pendingLabel: string, elapsedMs: number): boolean {
  return phase === "capturing" && pendingLabel.trim() === "" && elapsedMs >= CAPTURE_SILENCE_MS;
}

export function beginCapture(editor: HotkeyEditor): HotkeyEditor {
  return {
    ...editor,
    phase: "capturing",
    pendingChord: "",
    pendingLabel: "",
  };
}

export function showCaptured(editor: HotkeyEditor, chord: string, label: string): HotkeyEditor {
  return {
    ...editor,
    phase: "pending",
    pendingChord: chord,
    pendingLabel: label,
  };
}

export function cancelEditor(editor: HotkeyEditor): HotkeyEditor {
  return idleEditor(editor.activeChord, editor.activeLabel);
}

export function markApplied(editor: HotkeyEditor): HotkeyEditor {
  return idleEditor(editor.pendingChord, editor.pendingLabel);
}

export function knownConflict(chord: string): string | null {
  if (chord === "AltRight") {
    return "Right Option / Right Alt is Typeless's dictate key, including a PC keyboard on a Mac. The newtype default is Right Ctrl.";
  }
  if (chord === "AltRight+ShiftRight") {
    return "Right Option + Right Shift is a Typeless shortcut. The newtype default is Right Ctrl + Right Shift.";
  }
  return null;
}

export function bindingError(chords: string[]): string | null {
  for (let index = 0; index < chords.length; index += 1) {
    if (!chords[index]) {
      return `${MODE_NAMES[index] ?? "Hotkey"} hotkey is empty.`;
    }
    for (let other = index + 1; other < chords.length; other += 1) {
      if (chords[index] === chords[other]) {
        return `${MODE_NAMES[index]} and ${MODE_NAMES[other]} use the same hotkey.`;
      }
    }
  }
  return null;
}

export function bindingWarnings(chords: string[]): string[] {
  const warnings: string[] = [];
  const sets = chords.map((chord) => new Set(chord.split("+").filter(Boolean)));
  for (let index = 0; index < chords.length; index += 1) {
    for (let other = index + 1; other < chords.length; other += 1) {
      if (!chords[index] || !chords[other] || chords[index] === chords[other]) {
        continue;
      }
      const left = sets[index];
      const right = sets[other];
      const leftInside = [...left].every((key) => right.has(key));
      const rightInside = [...right].every((key) => left.has(key));
      if (leftInside || rightInside) {
        const short = left.size <= right.size ? chords[index] : chords[other];
        const long = left.size <= right.size ? chords[other] : chords[index];
        warnings.push(
          `${short} is also part of ${long}. A short press uses the shorter one; adding the extra key at the start uses the longer one.`,
        );
      }
    }
    const conflict = knownConflict(chords[index]);
    if (conflict) {
      warnings.push(conflict);
    }
  }
  return warnings;
}
