// Accelerators are stored the way Tauri parses them, e.g. "CommandOrControl+Alt+KeyT".
// Keys are physical codes (KeyboardEvent.code), so they work with any layout.

export const isMac = navigator.userAgent.includes("Mac");

const MAC_SYMBOLS: Record<string, string> = {
  CommandOrControl: "⌘",
  Command: "⌘",
  Super: "⌘",
  Control: "⌃",
  Alt: "⌥",
  Shift: "⇧",
};

const PC_NAMES: Record<string, string> = {
  CommandOrControl: "Ctrl",
  Control: "Ctrl",
  Command: "Win",
  Super: "Win",
  Alt: "Alt",
  Shift: "Shift",
};

const KEY_NAMES: Record<string, string> = {
  Space: "Space",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Enter: "↵",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
};

function keyLabel(code: string): string {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  return KEY_NAMES[code] ?? code;
}

/** One-line label for an accelerator: "⌘⌥T" on a Mac, "Ctrl+Alt+T" elsewhere. */
export function hotkeyLabel(accelerator: string): string {
  return hotkeyParts(accelerator).join(isMac ? "" : "+");
}

/** Keycap labels for an accelerator, in display order. */
export function hotkeyParts(accelerator: string): string[] {
  if (!accelerator) return [];
  const names = isMac ? MAC_SYMBOLS : PC_NAMES;
  return accelerator.split("+").map((part) => names[part] ?? keyLabel(part));
}

const MODIFIER_KEYS = new Set(["Control", "Shift", "Alt", "Meta", "OS", "AltGraph"]);

export type Recorded =
  | { kind: "cancel" }
  | { kind: "clear" }
  | { kind: "pending"; parts: string[] }
  | { kind: "invalid" }
  | { kind: "done"; accelerator: string };

/** Turns a keydown during recording into an accelerator, or tells why it isn't one yet. */
export function recordKey(e: KeyboardEvent): Recorded {
  const mods: string[] = [];
  if (isMac ? e.metaKey : e.ctrlKey) mods.push("CommandOrControl");
  if (isMac && e.ctrlKey) mods.push("Control");
  if (!isMac && e.metaKey) mods.push("Super");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");

  if (!mods.length && e.key === "Escape") return { kind: "cancel" };
  if (!mods.length && (e.key === "Backspace" || e.key === "Delete")) return { kind: "clear" };
  if (MODIFIER_KEYS.has(e.key)) return { kind: "pending", parts: hotkeyParts(mods.join("+")) };

  const functionKey = /^F\d{1,2}$/.test(e.code);
  const strongMods = mods.filter((m) => m !== "Shift");
  // A plain letter (or Shift+letter) would fire while typing, so require a real modifier.
  if (!functionKey && strongMods.length === 0) return { kind: "invalid" };
  return { kind: "done", accelerator: [...mods, e.code].join("+") };
}
