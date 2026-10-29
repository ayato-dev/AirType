/**
 * Errors get the spellchecker squiggle under the part that says what went wrong; the rest of the
 * message (after the first full stop, colon or comma) says what to do and stays plain.
 */
export function squiggleLead(text: string): (Node | string)[] {
  const match = /[.!?:,]\s/.exec(text);
  const cut = match ? match.index + (/[.!?]/.test(match[0][0]) ? 1 : 0) : text.length;
  const flag = document.createElement("span");
  flag.className = "flag";
  flag.textContent = text.slice(0, cut);
  return [flag, text.slice(cut)];
}
