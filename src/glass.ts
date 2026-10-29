// Windows (acrylic) and macOS (vibrancy) put glass behind the small windows; Linux
// has none, so there the page draws a paper sheet of its own.
export type Glass = "native" | "sheet";

export function glass(): Glass {
  const kind: Glass = /Windows|Macintosh/.test(navigator.userAgent) ? "native" : "sheet";
  document.documentElement.dataset.glass = kind;
  return kind;
}
