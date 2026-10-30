import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { squiggleLead } from "./flag";
import { glass } from "./glass";

type Mode = "compose" | "selection" | "answer";
type Phase = "ask" | "working" | "result" | "error";

interface View {
  seq: number;
  mode: Mode;
  text: string;
  answer: string;
  note: string | null;
  writable: boolean;
  actions: { id: string; name: string }[];
  targetLang: string;
  secondaryLang: string;
}

interface Ask {
  /** An action id, "custom" or "compose". */
  action: string;
  lang: string;
  tone: string;
  request: string;
}

interface Word {
  value: string;
  label: string;
}

const LANGS = [
  "English",
  "Русский",
  "Українська",
  "Deutsch",
  "Español",
  "Français",
  "Italiano",
  "Português",
  "Polski",
  "Türkçe",
  "中文",
  "日本語",
  "한국어",
];

// The value goes to the model, the label is what the panel says.
const TONES: Word[] = [
  { value: "", label: "обычно" },
  { value: "polite", label: "вежливо" },
  { value: "warm and friendly", label: "дружелюбно" },
  { value: "formal, businesslike", label: "по-деловому" },
  { value: "brief, to the point", label: "коротко" },
  { value: "light, with a bit of humour", label: "с юмором" },
];

const WORKING: Record<string, string> = {
  translate: "Перевожу",
  fix: "Исправляю",
  improve: "Улучшаю",
  compose: "Пишу",
};

const panel = document.getElementById("panel") as HTMLElement;
/** Linux draws its own sheet inside the window and needs room for the shadow. */
const margin = glass() === "sheet" ? 12 : 0;

let view: View | null = null;
let phase: Phase = "ask";
let answer = "";
let error = "";
let lastAsk: Ask | null = null;
/** Which word's choices are open under the sentence. */
let picking: "lang" | "composeLang" | "tone" | null = null;
/** The language picked for this selection; empty until the user picks one. */
let lang = "";
/** Kept between openings: the user's last picks for "write for me". */
let composeLang = "";
let tone = "";
let request = "";
/** Bumped on every request and on close, so a late answer to an old one is dropped. */
let run = 0;
let copied = false;

// ---------- small builders ----------

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
  ...children: (Node | string)[]
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  node.append(...children);
  return node;
}

function button(className: string, label: string, onClick: () => void): HTMLButtonElement {
  const b = el("button", className, label);
  b.type = "button";
  b.addEventListener("click", onClick);
  return b;
}

function comma(): HTMLElement {
  return el("span", "plain comma", ",");
}

// The verbs after the first read on in the sentence, so they start in lower case
// (an abbreviation like "SEO" stays as it is).
function inSentence(name: string): string {
  return name.length > 1 && name[1] === name[1].toLowerCase() ? name[0].toLowerCase() + name.slice(1) : name;
}

/** A word in the sentence that can be changed: it opens its choices under the line. */
function word(which: typeof picking, label: string): HTMLButtonElement {
  const b = button("word", label, () => {
    picking = picking === which ? null : which;
    render();
  });
  b.setAttribute("aria-expanded", String(picking === which));
  return b;
}

function choices(items: Word[], current: string, pick: (value: string) => void): HTMLElement {
  const list = el("div", "choices");
  list.setAttribute("role", "listbox");
  for (const item of items) {
    const b = button("choice", item.label, () => pick(item.value));
    b.setAttribute("role", "option");
    b.setAttribute("aria-selected", String(item.value === current));
    list.append(b);
  }
  return list;
}

type Script = "latin" | "cyrillic" | "cjk" | null;

function script(text: string): Script {
  const count = (re: RegExp) => (text.match(re) ?? []).length;
  const counts: [Script, number][] = [
    ["latin", count(/[a-zA-ZÀ-ÿĀ-ž]/g)],
    ["cyrillic", count(/[Ѐ-ӿ]/g)],
    ["cjk", count(/[\u3040-\u30ff\u3400-\u9fff\uac00-\ud7af]/g)],
  ];
  const [best, n] = counts.reduce((a, b) => (b[1] > a[1] ? b : a));
  return n > 0 ? best : null;
}

/**
 * What "Перевести на …" should say before the user picks: the target language,
 * or the second one when the text is plainly written in the target already
 * (an English page read with English as the target is read in Russian).
 */
function autoTarget(v: View): string {
  // A language's own name is written in its script.
  const target = script(v.targetLang);
  return target && script(v.text) === target && script(v.secondaryLang) !== target ? v.secondaryLang : v.targetLang;
}

function langWords(auto?: string): Word[] {
  const words = LANGS.map((l) => ({ value: l, label: l }));
  return auto === undefined ? words : [{ value: "", label: auto }, ...words];
}

// ---------- the parts ----------

function quote(text: string, lines: number): HTMLElement {
  const q = el("p", "quote", el("mark", "", text.trim()));
  q.style.setProperty("--lines", String(lines));
  return q;
}

function selectionAsk(v: View): HTMLElement[] {
  const verbs = el("div", "verbs");
  verbs.setAttribute("role", "toolbar");
  const current = lang || autoTarget(v);
  // One sentence: "Перевести на English, исправить, улучшить или вежливее".
  const last = v.actions.length - 1;
  v.actions.forEach((action, i) => {
    const group = el("span", "verb-group");
    if (i > 0 && i === last) group.append(el("span", "plain", "или "));
    const name = action.id === "translate" ? "Перевести" : action.name || "Без названия";
    group.append(button("verb", i > 0 ? inSentence(name) : name, () => ask({ action: action.id, lang: current, tone: "", request: "" })));
    if (action.id === "translate") group.append(el("span", "plain", " на "), word("lang", current));
    // The comma stays with the verb before it, so a line never starts with one.
    if (i < last - 1) group.append(comma());
    verbs.append(group);
  });

  const parts: HTMLElement[] = [quote(v.text, 2), verbs];
  if (picking === "lang") {
    // Picking a language means "translate into it", so it runs right away.
    parts.push(
      choices(langWords(), current, (value) => {
        lang = value;
        picking = null;
        ask({ action: "translate", lang: value, tone: "", request: "" });
      }),
    );
  }

  const own = el("input", "input");
  own.id = "own";
  own.placeholder = "Или скажи, что сделать с текстом";
  own.spellcheck = false;
  own.value = request;
  own.addEventListener("input", () => (request = own.value));
  own.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && own.value.trim()) {
      e.preventDefault();
      ask({ action: "custom", lang: current, tone: "", request: own.value.trim() });
    }
  });
  parts.push(own);
  return parts;
}

function composeAsk(): HTMLElement[] {
  const area = el("textarea", "input request");
  area.id = "request";
  area.rows = 1;
  area.placeholder = "Что написать? Например: вежливо откажись от встречи в пятницу";
  area.spellcheck = false;
  area.value = request;
  area.addEventListener("input", () => {
    request = area.value;
    grow(area);
    void fit();
  });
  area.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submitCompose();
    }
  });

  const toneLabel = TONES.find((t) => t.value === tone)?.label ?? TONES[0].label;
  const sentence = el(
    "p",
    "sentence",
    el("span", "plain", "На "),
    word("composeLang", composeLang || "языке запроса"),
    comma(),
    word("tone", toneLabel),
  );
  const go = button("btn-mark", "Написать", submitCompose);
  const bar = el("div", "bar", sentence, go);

  const parts: HTMLElement[] = [area, bar];
  if (picking === "composeLang") {
    parts.push(
      choices(langWords("языке запроса"), composeLang, (value) => {
        composeLang = value;
        picking = null;
        render();
      }),
    );
  }
  if (picking === "tone") {
    parts.push(
      choices(TONES, tone, (value) => {
        tone = value;
        picking = null;
        render();
      }),
    );
  }
  return parts;
}

function submitCompose() {
  if (!request.trim()) {
    document.getElementById("request")?.focus();
    return;
  }
  ask({ action: "compose", lang: composeLang, tone, request: request.trim() });
}

function working(): HTMLElement {
  const words = `${WORKING[lastAsk?.action ?? ""] ?? "Думаю"}…`;
  const line = el("p", "working");
  const text = el("span", "working-text", words);
  text.dataset.text = words;
  line.append(text);
  return line;
}

function result(v: View): HTMLElement[] {
  const parts: HTMLElement[] = [];
  if (v.mode === "answer" && v.note) parts.push(el("p", "note", ...squiggleLead(v.note)));
  if (v.mode !== "compose" && v.text.trim()) parts.push(quote(v.text, 1));
  const sheet = el("div", "answer", answer);
  sheet.tabIndex = 0;
  parts.push(sheet);

  const bar = el("div", "bar");
  const tools = el("div", "tools");
  const copy = button("link", copied ? "Скопировано" : "Скопировать", copyAnswer);
  const writable = v.mode === "compose" || (v.mode === "selection" && v.writable);
  if (writable) {
    const put = button("btn-mark", v.mode === "compose" ? "Вставить" : "Заменить", write);
    put.id = "primary";
    bar.append(put);
    tools.append(copy);
  } else {
    const main = button("btn-mark", copied ? "Скопировано" : "Скопировать", copyAnswer);
    main.id = "primary";
    bar.append(main);
  }
  if (lastAsk && v.mode !== "answer") {
    tools.append(button("link", "Ещё раз", () => lastAsk && ask(lastAsk)));
    tools.append(button("link", v.mode === "compose" ? "Изменить" : "Назад", back));
  }
  bar.append(tools);
  parts.push(bar);
  return parts;
}

function failure(): HTMLElement[] {
  const line = el("p", "note", ...squiggleLead(error));
  const bar = el("div", "bar");
  const again = button("btn-mark", "Ещё раз", () => lastAsk && ask(lastAsk));
  again.id = "primary";
  bar.append(again, el("div", "tools", button("link", "Назад", back)));
  return [line, bar];
}

// ---------- flow ----------

function render() {
  if (!view) return;
  const v = view;
  panel.dataset.mode = v.mode;
  panel.dataset.phase = phase;
  switch (phase) {
    case "ask":
      panel.replaceChildren(...(v.mode === "compose" ? composeAsk() : selectionAsk(v)));
      break;
    case "working":
      panel.replaceChildren(v.mode === "compose" ? el("p", "asked", request.trim()) : quote(v.text, 1), working());
      break;
    case "result":
      panel.replaceChildren(...result(v));
      break;
    case "error":
      panel.replaceChildren(...failure());
      break;
  }
  const area = document.getElementById("request") as HTMLTextAreaElement | null;
  if (area) grow(area);
  focusMain();
  void fit();
}

function grow(area: HTMLTextAreaElement) {
  area.style.height = "auto";
  area.style.height = `${Math.min(area.scrollHeight, 126)}px`;
}

function focusMain() {
  if (picking) {
    panel.querySelector<HTMLElement>('.choice[aria-selected="true"], .choice')?.focus();
    return;
  }
  const target =
    document.getElementById("primary") ??
    document.getElementById("request") ??
    panel.querySelector<HTMLElement>(".verb");
  target?.focus({ preventScroll: true });
}

/** The window takes the panel's height; the app places it at the text and shows it. */
async function fit() {
  if (!view) return;
  const seq = view.seq;
  await document.fonts.ready;
  await new Promise(requestAnimationFrame);
  if (view?.seq !== seq) return;
  const height = Math.ceil(panel.getBoundingClientRect().height) + margin * 2;
  await invoke("panel_fit", { seq, height });
}

async function ask(next: Ask) {
  lastAsk = next;
  phase = "working";
  picking = null;
  copied = false;
  render();
  const mine = ++run;
  try {
    const text = await invoke<string>("panel_run", { ask: next });
    if (mine !== run) return;
    answer = text;
    phase = "result";
  } catch (e) {
    if (mine !== run) return;
    error = String(e);
    phase = "error";
  }
  render();
}

function back() {
  run++;
  // Nobody waits for that answer any more, so it shouldn't keep running (or cost money).
  if (phase === "working") void invoke("panel_cancel");
  phase = "ask";
  render();
}

function write() {
  run++;
  view = null;
  void invoke("panel_write", { text: answer });
}

async function copyAnswer() {
  try {
    await invoke("panel_copy", { text: answer });
  } catch (e) {
    error = String(e);
    phase = "error";
    render();
    return;
  }
  copied = true;
  render();
  window.setTimeout(() => close(true), 450);
}

function close(restore: boolean) {
  run++;
  view = null;
  void invoke("panel_close", { restore });
}

async function load() {
  const next = await invoke<View | null>("panel_view");
  if (!next || next.seq === view?.seq) return;
  view = next;
  run++;
  phase = next.mode === "answer" ? "result" : "ask";
  answer = next.answer;
  error = "";
  lastAsk = null;
  picking = null;
  lang = "";
  request = "";
  copied = false;
  render();
}

window.addEventListener("keydown", (e) => {
  if (!view) return;
  if (e.key === "Escape") {
    e.preventDefault();
    if (picking) {
      picking = null;
      render();
    } else {
      close(true);
    }
    return;
  }
  const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
  if (e.key === "Enter" && !typing && (phase === "result" || phase === "error")) {
    const primary = document.getElementById("primary");
    if (primary && e.target !== primary) {
      e.preventDefault();
      primary.click();
    }
    return;
  }
  // Arrows walk the words of a sentence, like a toolbar.
  if ((e.key === "ArrowRight" || e.key === "ArrowLeft") && !typing) {
    const words = [...panel.querySelectorAll<HTMLElement>(picking ? ".choice" : ".verb, .word")];
    const at = words.indexOf(document.activeElement as HTMLElement);
    if (at < 0) return;
    e.preventDefault();
    const next = (at + (e.key === "ArrowRight" ? 1 : words.length - 1)) % words.length;
    words[next].focus();
  }
});

// The window gets focus only once it is shown; the field inside needs it again then.
window.addEventListener("focus", focusMain);

void listen<number>("panel-open", () => void load());
void load();
