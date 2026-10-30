import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { combo as comboBox, menuOpen, select, type Choice } from "./dropdown";
import { squiggleLead } from "./flag";
import { hotkeyLabel, hotkeyParts, isMac, recordKey } from "./hotkey";

type Provider = "anthropic" | "openai" | "local";
type View = "actions" | "model" | "general";

interface Action {
  id: string;
  name: string;
  hotkey: string;
  prompt: string;
  enabled: boolean;
}

interface ProviderConfig {
  model: string;
  baseUrl: string;
}

interface PanelConfig {
  hotkey: string;
  enabled: boolean;
}

interface Settings {
  provider: Provider;
  anthropic: ProviderConfig;
  openai: ProviderConfig;
  local: { model: string; gpu: boolean };
  effort: string;
  targetLang: string;
  secondaryLang: string;
  showHud: boolean;
  paused: boolean;
  actions: Action[];
  panel: PanelConfig;
}

interface HotkeyError {
  actionId: string;
  message: string;
}

interface UiState {
  settings: Settings;
  keys: Record<"anthropic" | "openai", boolean>;
  hotkeyErrors: HotkeyError[];
  /** The settings file couldn't be read and was set aside. */
  notice: string | null;
  platform: string;
  rounded: boolean;
  wayland: boolean;
  localSupported: boolean;
  accessibility: boolean;
  autostart: boolean;
  version: string;
}

interface LocalModel {
  id: string;
  name: string;
  note: string;
  size: number;
  downloaded: boolean;
}

interface LocalProgress {
  model: string;
  stage: "server" | "model" | "check" | "done" | "error" | "cancelled";
  done: number;
  total: number;
  message: string | null;
}

interface LocalStatus {
  models: LocalModel[];
  download: LocalProgress | null;
  /** The engine here can run on the video card. */
  gpu: boolean;
}

type CloudProvider = "anthropic" | "openai";

const BUILTIN = new Set(["translate", "fix", "improve"]);
const PANEL_ID = "panel";
const VIEWS: View[] = ["actions", "model", "general"];

const MODELS: Record<CloudProvider, { value: string; label: string }[]> = {
  anthropic: [
    { value: "claude-opus-5-5", label: "Самая умная" },
    { value: "claude-sonnet-5-5", label: "Баланс" },
    { value: "claude-haiku-5-5", label: "Самая быстрая и дешёвая" },
  ],
  openai: [],
};

const EFFORTS: Choice[] = [
  { value: "low", label: "Низкое, быстрее" },
  { value: "medium", label: "Среднее" },
  { value: "high", label: "Высокое, дольше" },
  { value: "", label: "Как у модели" },
];

const LANGS: Choice[] = [
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
].map((value) => ({ value }));

const PROVIDER_TEXT: Record<
  CloudProvider,
  { hint: string; key: string; model: string; url: string; urlHint: string }
> = {
  anthropic: {
    hint: "Ключ берётся в console.anthropic.com, раздел API keys.",
    key: "sk-ant-…",
    model: "claude-haiku-5-5",
    url: "https://api.anthropic.com",
    urlHint: "Пусто значит официальный api.anthropic.com. Для прокси укажи адрес без /v1.",
  },
  openai: {
    hint: "OpenAI, OpenRouter, Ollama, LM Studio и всё, что говорит на языке OpenAI.",
    key: "Для локальной модели не нужен",
    model: "Имя модели у провайдера",
    url: "https://api.openai.com/v1",
    urlHint: "Пусто значит api.openai.com/v1. OpenRouter: https://openrouter.ai/api/v1, Ollama: http://localhost:11434/v1",
  },
};

// Examples for the preview. They show what an action does; the real answer comes from the model.
const SAMPLE_RU = "Привет! Скинь, пожалуйста, файл, когда будет минутка.";
const SAMPLE_EN = "Hey! Could you send me the file when you have a minute?";
const TRANSLATED: Record<string, string> = {
  english: "Hi! Could you send me the file when you have a minute?",
  deutsch: "Hi! Kannst du mir die Datei schicken, wenn du kurz Zeit hast?",
  español: "¡Hola! ¿Me pasas el archivo cuando tengas un momento?",
  français: "Salut ! Tu peux m’envoyer le fichier quand tu auras une minute ?",
  italiano: "Ciao! Mi mandi il file quando hai un minuto?",
  português: "Oi! Pode me mandar o arquivo quando tiver um minuto?",
  polski: "Cześć! Podeślesz mi plik, jak znajdziesz chwilę?",
  українська: "Привіт! Скинь, будь ласка, файл, коли буде хвилинка.",
};

const SVG_NS = "http://www.w3.org/2000/svg";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const el = {
  notes: $("notes"),
  views: document.querySelector<HTMLElement>(".views")!,
  tabs: document.querySelector<HTMLElement>(".tabs")!,
  tabButtons: [...document.querySelectorAll<HTMLButtonElement>(".tab")],
  previewBefore: $("preview-before"),
  previewKeys: $("preview-keys"),
  previewAfter: $("preview-after"),
  actions: $<HTMLUListElement>("actions"),
  addAction: $<HTMLButtonElement>("add-action"),
  targetLang: $<HTMLInputElement>("target-lang"),
  secondaryLang: $<HTMLInputElement>("secondary-lang"),
  providers: [...document.querySelectorAll<HTMLInputElement>('input[name="provider"]')],
  providerHint: $("provider-hint"),
  apiKey: $<HTMLInputElement>("api-key"),
  keyEye: $<HTMLButtonElement>("key-eye"),
  keyState: $("key-state"),
  model: $<HTMLInputElement>("model"),
  effortField: $("effort-field"),
  form: $("form"),
  local: $("local"),
  localModels: $<HTMLUListElement>("local-models"),
  gpuRow: $("gpu-row"),
  gpu: $<HTMLInputElement>("gpu"),
  localOption: $("local-option"),
  localOr: $("local-or"),
  providerOr: $("provider-or"),
  panelRow: $<HTMLUListElement>("panel-row"),
  effort: $<HTMLButtonElement>("effort"),
  baseUrl: $<HTMLInputElement>("base-url"),
  baseUrlHint: $("base-url-hint"),
  test: $<HTMLButtonElement>("test"),
  testResult: $("test-result"),
  testBefore: $("test-before"),
  testAfter: $("test-after"),
  enabled: $<HTMLInputElement>("enabled"),
  showHud: $<HTMLInputElement>("show-hud"),
  autostart: $<HTMLInputElement>("autostart"),
  axGroup: $("ax-group"),
  axState: $("ax-state"),
  axOpen2: $<HTMLButtonElement>("ax-open-2"),
  version: $("version"),
};

let ui: UiState;
let settings: Settings;
let view: View = "actions";
let expanded: string | null = null;
let previewId: string | null = null;
/** Whose hotkey is being recorded: an action or the panel. */
let recording: { owner: { hotkey: string }; chip: HTMLButtonElement } | null = null;
let local: LocalStatus = { models: [], download: null, gpu: false };
let effortSelect: { render: () => void } | null = null;

function icon(name: string): SVGSVGElement {
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("class", "icon");
  svg.setAttribute("aria-hidden", "true");
  const use = document.createElementNS(SVG_NS, "use");
  use.setAttribute("href", `#${name}`);
  svg.append(use);
  return svg;
}

function requiresKey(): boolean {
  if (settings.provider === "local") return false;
  return settings.provider === "anthropic" || settings.openai.baseUrl.trim() === "";
}

function hasKey(): boolean {
  return settings.provider !== "local" && ui.keys[settings.provider];
}

function localReady(): boolean {
  return local.models.some((m) => m.id === settings.local.model && m.downloaded);
}

// ---------- saving ----------

let saveTimer: number | undefined;

function scheduleSave(delay = 350) {
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(save, delay);
}

async function save() {
  window.clearTimeout(saveTimer);
  try {
    ui.hotkeyErrors = await invoke<HotkeyError[]>("save_settings", { settings });
    // The new file is written, so the remark about the old one is done.
    if (ui.notice) {
      ui.notice = null;
      renderNotes();
    }
    refreshActionMeta();
  } catch (e) {
    complain("Не сохранилось.", e);
  }
}

// ---------- views ----------

function showView(next: View, focus = false) {
  view = next;
  for (const tab of el.tabButtons) {
    const on = tab.dataset.view === view;
    tab.setAttribute("aria-selected", String(on));
    tab.tabIndex = on ? 0 : -1;
    if (on && focus) tab.focus();
  }
  for (const name of VIEWS) $(`view-${name}`).hidden = name !== view;
  el.views.scrollTop = 0;
  renderNotes();
  updateFade();
}

function updateFade() {
  const v = el.views;
  v.classList.toggle("more-above", v.scrollTop > 2);
  v.classList.toggle("more-below", v.scrollTop + v.clientHeight < v.scrollHeight - 2);
}

// ---------- rendering ----------

function keycaps(accelerator: string): HTMLElement[] {
  return hotkeyParts(accelerator).map((part) => {
    const cap = document.createElement("kbd");
    cap.className = "kbd";
    cap.textContent = part;
    return cap;
  });
}

// ---------- notes: remarks in the margin ----------

interface Note {
  text: string;
  /** The words a proofreader would squiggle: what is wrong. */
  flag?: string;
  action?: { label: string; run: () => void };
}

let complaint: Note | null = null;
let complaintTimer: number | undefined;

/** Something failed: say so above the settings for a few seconds. */
function complain(what: string, error: unknown) {
  const detail = String(error);
  complaint = { text: `${what} ${detail.charAt(0).toUpperCase()}${detail.slice(1)}`, flag: what };
  window.clearTimeout(complaintTimer);
  complaintTimer = window.setTimeout(() => {
    complaint = null;
    renderNotes();
  }, 6000);
  renderNotes();
}

function openAccessibility() {
  void invoke("request_accessibility");
}

function currentNotes(): Note[] {
  const notes: Note[] = [];
  if (complaint) notes.push(complaint);
  if (ui.notice) notes.push({ text: ui.notice, flag: "не прочитался" });
  // On the model tab the key field is right there, a remark about it would only repeat it.
  if (requiresKey() && !hasKey() && view !== "model") {
    notes.push({
      text: "Без ключа API нейронка не ответит.",
      flag: "ключа API",
      action: {
        label: "Вставить ключ",
        run: () => {
          showView("model");
          el.apiKey.focus();
        },
      },
    });
  }
  if (settings.provider === "local" && !localReady() && view !== "model") {
    notes.push({
      text: "Модель на компьютере ещё не скачана.",
      flag: "не скачана",
      action: { label: "Скачать", run: () => showView("model") },
    });
  }
  if (ui.wayland) {
    notes.push({
      text: "Это сессия Wayland, и сочетания тут сработают только в X11-приложениях. Для остальных выбери Xorg на экране входа.",
      flag: "только в X11-приложениях",
    });
  }
  if (ui.platform === "macos" && !ui.accessibility && view !== "general") {
    notes.push({
      text: "macOS пока не даёт Airtype копировать и вставлять текст.",
      flag: "не даёт",
      action: { label: "Разрешить", run: openAccessibility },
    });
  }
  // On the general tab the "hotkeys work" tick already says it.
  if (settings.paused && view !== "general") {
    notes.push({ text: "Сочетания на паузе.", action: { label: "Включить", run: () => setPaused(false) } });
  }
  return notes;
}

function renderNotes() {
  el.notes.replaceChildren(
    ...currentNotes().map((note) => {
      const p = document.createElement("p");
      p.className = "note";
      const at = note.flag ? note.text.indexOf(note.flag) : -1;
      if (note.flag && at >= 0) {
        const flag = Object.assign(document.createElement("span"), { className: "flag", textContent: note.flag });
        p.append(note.text.slice(0, at), flag, note.text.slice(at + note.flag.length));
      } else {
        p.append(note.text);
      }
      if (note.action) {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "link";
        button.textContent = note.action.label;
        button.addEventListener("click", note.action.run);
        p.append(" ", button);
      }
      return p;
    }),
  );
}

function renderStatus() {
  el.enabled.checked = !settings.paused;
  el.axGroup.hidden = ui.platform !== "macos";
  el.axState.textContent = ui.accessibility
    ? "Разрешён, Airtype копирует и вставляет текст за тебя."
    : "Нужен, чтобы Airtype мог скопировать текст из поля и вставить ответ.";
  el.axState.classList.toggle("ok", ui.accessibility);
  el.axOpen2.hidden = ui.accessibility;
  renderNotes();
}

function translateExample(): { before: string; after: string | null } {
  const target = settings.targetLang.trim().toLowerCase();
  if (target === "русский" || target === "russian") return { before: SAMPLE_EN, after: SAMPLE_RU };
  return { before: SAMPLE_RU, after: TRANSLATED[target] ?? null };
}

function example(action: Action): { before: string; after: string | null } {
  switch (action.id) {
    case "translate":
      return translateExample();
    case "fix":
      return {
        before: "привет скинь пожалуста файл когда будет минутка",
        after: "Привет, скинь, пожалуйста, файл, когда будет минутка.",
      };
    case "improve":
      return {
        before: "привет, скинь файл как сможешь",
        after: "Привет! Пришли, пожалуйста, файл, когда будет удобно.",
      };
    default:
      return { before: SAMPLE_RU, after: null };
  }
}

function renderPreview(animate = false) {
  const action =
    settings.actions.find((a) => a.id === previewId) ??
    settings.actions.find((a) => a.enabled && a.hotkey) ??
    settings.actions[0];
  if (!action) return;

  const { before, after } = example(action);
  el.previewBefore.textContent = before;

  if (action.hotkey) {
    el.previewKeys.replaceChildren(...keycaps(action.hotkey));
  } else {
    const none = document.createElement("span");
    none.className = "hint";
    none.textContent = "Сочетание не задано";
    el.previewKeys.replaceChildren(none);
  }

  el.previewAfter.className = "preview-after";
  if (after) {
    el.previewAfter.textContent = after;
  } else {
    el.previewAfter.classList.add("muted");
    el.previewAfter.textContent =
      action.id === "translate"
        ? `Переведёт на ${settings.targetLang.trim() || "выбранный язык"}`
        : `Нейронка сделает то, что написано в «${action.name || "Без названия"}»`;
  }
  if (animate) {
    void el.previewAfter.offsetWidth;
    el.previewAfter.classList.add("swap");
  }
}

function renderProvider() {
  const provider = settings.provider;
  for (const radio of el.providers) radio.checked = radio.value === provider;
  el.localOption.hidden = el.localOr.hidden = !ui.localSupported && provider !== "local";
  // Three choices read "Claude, OpenAI-compatible or the one on this computer".
  el.providerOr.textContent = el.localOption.hidden ? "или" : ",";
  el.providerOr.classList.toggle("comma", !el.localOption.hidden);
  el.form.hidden = provider === "local";
  el.local.hidden = provider !== "local";
  el.testResult.hidden = true;
  el.providerHint.hidden = provider === "local";
  if (provider === "local") {
    renderLocal();
    renderStatus();
    return;
  }
  const cfg = settings[provider];
  const text = PROVIDER_TEXT[provider];
  el.providerHint.textContent = text.hint;

  const hasKey = ui.keys[provider];
  el.apiKey.value = "";
  el.apiKey.placeholder = hasKey ? "Вставь новый, чтобы заменить" : text.key;
  el.keyState.textContent = hasKey ? "сохранён" : "";

  el.model.value = cfg.model;
  el.model.placeholder = text.model;

  el.effortField.hidden = provider !== "anthropic";
  effortSelect?.render();
  el.baseUrl.value = cfg.baseUrl;
  el.baseUrl.placeholder = text.url;
  el.baseUrlHint.textContent = text.urlHint;
  renderStatus();
}

// ---------- the model on this computer ----------

function size(bytes: number): string {
  return `${(bytes / 1e9).toLocaleString("ru-RU", { maximumFractionDigits: 1 })} ГБ`;
}

function localRow(model: LocalModel): HTMLLIElement {
  const li = document.createElement("li");
  const row = document.createElement("div");
  row.className = "row local-model";
  const progress = local.download?.model === model.id ? local.download : null;
  const chosen = model.downloaded && settings.local.model === model.id;
  row.toggleAttribute("data-chosen", chosen);

  const main = document.createElement("button");
  main.type = "button";
  main.className = "action-main";
  main.disabled = !model.downloaded;
  const title = document.createElement("span");
  title.className = "row-title";
  const name = document.createElement("span");
  name.className = "local-name";
  name.textContent = model.name;
  name.dataset.text = model.name;
  // While it downloads, the highlighter runs through the name as far as it has come.
  if (progress && progress.total > 0) {
    name.style.setProperty("--done", `${Math.round((progress.done / progress.total) * 100)}%`);
    name.classList.add("loading");
  }
  title.append(name);
  const desc = document.createElement("span");
  desc.className = "row-desc";
  desc.textContent = `${model.note}. ${size(model.size)} на диске`;
  main.append(title, desc);
  main.addEventListener("click", () => {
    if (!model.downloaded || chosen) return;
    settings.local.model = model.id;
    renderLocal();
    renderNotes();
    scheduleSave(0);
  });

  const side = document.createElement("div");
  side.className = "local-side";
  const link = (label: string, run: () => void, danger = false) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = danger ? "link-danger" : "link";
    b.textContent = label;
    b.addEventListener("click", run);
    return b;
  };
  const say = (text: string) => Object.assign(document.createElement("span"), { className: "hint", textContent: text });

  if (progress) {
    const pct = progress.total > 0 ? Math.floor((progress.done / progress.total) * 100) : 0;
    const what =
      progress.stage === "server" ? `Ставлю движок ${pct}%` : progress.stage === "check" ? "Проверяю файл" : `Качаю ${pct}%`;
    side.append(say(what), link("Стоп", () => void invoke("local_cancel")));
  } else if (model.downloaded) {
    if (chosen) side.append(say("Выбрана"));
    side.append(
      link(
        "Удалить",
        async () => {
          try {
            await invoke("local_delete", { model: model.id });
            await refreshLocal();
          } catch (e) {
            complain("Не удалилось.", e);
          }
        },
        true,
      ),
    );
  } else {
    side.append(
      link("Скачать", async () => {
        try {
          await invoke("local_download", { model: model.id });
          await refreshLocal();
        } catch (e) {
          complain("Не качается.", e);
        }
      }),
    );
  }

  row.append(main, side);
  li.append(row);
  if (localError?.model === model.id) {
    const error = document.createElement("p");
    error.className = "local-error";
    error.append(...squiggleLead(localError.message));
    li.append(error);
  }
  return li;
}

let localError: { model: string; message: string } | null = null;

function renderLocal() {
  el.localModels.replaceChildren(...local.models.map(localRow));
  el.gpuRow.hidden = !local.gpu;
  el.gpu.checked = settings.local.gpu;
}

async function refreshLocal() {
  local = await invoke<LocalStatus>("local_status");
  if (settings.provider === "local") renderLocal();
  renderNotes();
}

function renderActions() {
  el.actions.replaceChildren(...settings.actions.map(actionRow));
  renderPanelRow();
  renderPreview();
}

function panelMeta(): { text: string; error: boolean } {
  const error = ui.hotkeyErrors.find((e) => e.actionId === PANEL_ID);
  if (error && settings.panel.enabled) return { text: error.message, error: true };
  if (!settings.panel.enabled) return { text: "Выключено", error: false };
  return {
    text: "В пустом поле напишет текст по твоей просьбе. С выделенным текстом, даже на сайте, переведёт, исправит или улучшит и покажет ответ",
    error: false,
  };
}

function renderPanelRow() {
  const li = document.createElement("li");
  li.className = "action";
  li.classList.toggle("off", !settings.panel.enabled);
  const head = document.createElement("div");
  head.className = "action-head";

  const toggle = document.createElement("input");
  toggle.type = "checkbox";
  toggle.className = "tick";
  toggle.checked = settings.panel.enabled;
  toggle.setAttribute("aria-label", "Включить панель");
  toggle.addEventListener("change", () => {
    settings.panel.enabled = toggle.checked;
    renderPanelRow();
    scheduleSave(0);
  });

  const main = document.createElement("div");
  main.className = "action-main";
  const title = Object.assign(document.createElement("span"), { className: "row-title", textContent: "Напиши за меня" });
  const desc = document.createElement("span");
  desc.className = "row-desc";
  desc.dataset.meta = PANEL_ID;
  const meta = panelMeta();
  desc.textContent = meta.text;
  desc.classList.toggle("flag", meta.error);
  main.append(title, desc);

  const chip = document.createElement("button");
  chip.type = "button";
  chip.className = "keys";
  chip.title = "Нажми и введи новое сочетание";
  chip.setAttribute("aria-label", "Сочетание для панели");
  fillChip(chip, settings.panel.hotkey);
  chip.addEventListener("click", () => startRecording(settings.panel, chip));

  head.append(toggle, main, chip);
  li.append(head);
  el.panelRow.replaceChildren(li);
}

function actionMeta(action: Action): { text: string; error: boolean } {
  const error = ui.hotkeyErrors.find((e) => e.actionId === action.id);
  if (error && action.enabled) return { text: error.message, error: true };
  if (!action.enabled) return { text: "Выключено", error: false };
  switch (action.id) {
    case "translate":
      return { text: `На ${settings.targetLang.trim()} или ${settings.secondaryLang.trim()}`, error: false };
    case "fix":
      return { text: "Опечатки, запятые, орфография", error: false };
    case "improve":
      return { text: "Понятнее и живее, смысл тот же", error: false };
    default:
      return { text: "Своя инструкция", error: false };
  }
}

function refreshActionMeta() {
  const panelDesc = el.panelRow.querySelector<HTMLElement>(`[data-meta="${PANEL_ID}"]`);
  if (panelDesc) {
    const meta = panelMeta();
    panelDesc.textContent = meta.text;
    panelDesc.classList.toggle("flag", meta.error);
  }
  for (const action of settings.actions) {
    const desc = el.actions.querySelector<HTMLElement>(`[data-meta="${action.id}"]`);
    if (!desc) continue;
    const meta = actionMeta(action);
    desc.textContent = meta.text;
    desc.classList.toggle("flag", meta.error);
  }
  renderPreview();
}

function actionRow(action: Action): HTMLLIElement {
  const li = document.createElement("li");
  li.className = "action";
  li.classList.toggle("off", !action.enabled);
  li.toggleAttribute("data-open", expanded === action.id);

  const head = document.createElement("div");
  head.className = "action-head";

  const main = document.createElement("button");
  main.type = "button";
  main.className = "action-main";
  main.setAttribute("aria-expanded", String(expanded === action.id));
  const title = document.createElement("span");
  title.className = "row-title";
  title.textContent = action.name || "Без названия";
  const desc = document.createElement("span");
  desc.className = "row-desc";
  desc.dataset.meta = action.id;
  const meta = actionMeta(action);
  desc.textContent = meta.text;
  desc.classList.toggle("flag", meta.error);
  main.append(title, desc);
  main.addEventListener("click", () => {
    expanded = expanded === action.id ? null : action.id;
    previewId = action.id;
    renderActions();
    renderPreview(true);
  });

  const chip = document.createElement("button");
  chip.type = "button";
  chip.className = "keys";
  chip.title = "Нажми и введи новое сочетание";
  chip.setAttribute("aria-label", `Сочетание для «${action.name}»`);
  fillChip(chip, action.hotkey);
  chip.addEventListener("click", () => startRecording(action, chip));

  const toggle = document.createElement("input");
  toggle.type = "checkbox";
  toggle.className = "tick";
  toggle.checked = action.enabled;
  toggle.setAttribute("aria-label", `Включить «${action.name}»`);
  toggle.addEventListener("change", () => {
    action.enabled = toggle.checked;
    li.classList.toggle("off", !action.enabled);
    scheduleSave(0);
    refreshActionMeta();
  });

  head.append(toggle, main, chip);
  li.append(head);

  if (expanded === action.id) li.append(actionEditor(action, title));
  return li;
}

function actionEditor(action: Action, title: HTMLElement): HTMLElement {
  const body = document.createElement("div");
  body.className = "action-editor";

  const nameInput = document.createElement("input");
  nameInput.className = "input";
  nameInput.value = action.name;
  nameInput.placeholder = "Название";
  nameInput.setAttribute("aria-label", "Название");
  nameInput.addEventListener("input", () => {
    action.name = nameInput.value;
    title.textContent = action.name || "Без названия";
    scheduleSave();
  });

  const prompt = document.createElement("textarea");
  prompt.className = "input";
  prompt.value = action.prompt;
  prompt.placeholder = "Что сделать с текстом, например: сделай вежливее и короче";
  prompt.setAttribute("aria-label", "Инструкция для нейронки");
  prompt.spellcheck = false;
  prompt.addEventListener("input", () => {
    action.prompt = prompt.value;
    scheduleSave();
  });

  const tools = document.createElement("div");
  tools.className = "action-tools";
  const hint = document.createElement("span");
  hint.className = "hint";
  hint.textContent = "{target} и {secondary} заменятся языками из раздела «Перевод»";
  tools.append(hint);

  if (!BUILTIN.has(action.id)) {
    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "link-danger";
    remove.textContent = "Удалить";
    remove.addEventListener("click", () => {
      settings.actions = settings.actions.filter((a) => a !== action);
      expanded = null;
      if (previewId === action.id) previewId = null;
      renderActions();
      scheduleSave(0);
    });
    tools.append(remove);
  }

  body.append(nameInput, prompt, tools);
  return body;
}

function combo(text: string): HTMLElement {
  return Object.assign(document.createElement("kbd"), { className: "combo", textContent: text });
}

function fillChip(chip: HTMLButtonElement, accelerator: string, placeholder = "Задать") {
  if (accelerator) {
    chip.replaceChildren(combo(hotkeyLabel(accelerator)));
  } else {
    const empty = document.createElement("span");
    empty.className = "empty";
    empty.textContent = placeholder;
    chip.replaceChildren(empty);
  }
}

// ---------- hotkey recording ----------

function startRecording(owner: { hotkey: string }, chip: HTMLButtonElement) {
  if (recording) stopRecording();
  recording = { owner, chip };
  chip.classList.add("recording");
  fillChip(chip, "", "Жми сочетание");
  // The current hotkeys would otherwise fire while the user presses them here.
  void invoke("suspend_hotkeys", { suspended: true });
}

function stopRecording() {
  if (!recording) return;
  const { owner, chip } = recording;
  recording = null;
  chip.classList.remove("recording");
  fillChip(chip, owner.hotkey);
  void save().then(async () => {
    ui.hotkeyErrors = await invoke<HotkeyError[]>("suspend_hotkeys", { suspended: false });
    refreshActionMeta();
  });
}

window.addEventListener(
  "keydown",
  (e) => {
    if (!recording) {
      // An open list takes Escape for itself.
      if (menuOpen()) return;
      const hideCombo = (e.metaKey || e.ctrlKey) && e.code === "KeyW";
      const isField = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if (hideCombo || (e.key === "Escape" && !isField)) {
        e.preventDefault();
        void invoke("hide_settings");
      }
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    const result = recordKey(e);
    const { owner, chip } = recording;
    switch (result.kind) {
      case "cancel":
        stopRecording();
        break;
      case "clear":
        owner.hotkey = "";
        stopRecording();
        break;
      case "pending":
        chip.replaceChildren(combo(result.parts.join(isMac ? "" : "+")));
        break;
      case "invalid":
        fillChip(chip, "", isMac ? "Нужен ⌘, ⌃ или ⌥" : "Нужен Ctrl или Alt");
        break;
      case "done":
        owner.hotkey = result.accelerator;
        stopRecording();
        break;
    }
  },
  true,
);

window.addEventListener("blur", () => stopRecording());

// ---------- wiring ----------

function setPaused(paused: boolean) {
  settings.paused = paused;
  renderStatus();
  void save();
}

function bind() {
  $("close").addEventListener("click", () => void invoke("hide_settings"));
  $("minimize").addEventListener("click", () => void getCurrentWindow().minimize());
  el.views.addEventListener("scroll", updateFade, { passive: true });
  const resized = new ResizeObserver(updateFade);
  resized.observe(el.views);
  for (const view of el.views.children) resized.observe(view);

  for (const tab of el.tabButtons) {
    tab.addEventListener("click", () => showView(tab.dataset.view as View));
  }
  el.tabs.addEventListener("keydown", (e) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    const current = VIEWS.findIndex((v) => !$(`view-${v}`).hidden);
    const next = (current + (e.key === "ArrowRight" ? 1 : VIEWS.length - 1)) % VIEWS.length;
    showView(VIEWS[next], true);
  });

  el.enabled.addEventListener("change", () => setPaused(!el.enabled.checked));

  for (const radio of el.providers) {
    radio.addEventListener("change", () => {
      settings.provider = radio.value as Provider;
      renderProvider();
      scheduleSave(0);
    });
  }

  el.keyEye.addEventListener("click", () => {
    const show = el.apiKey.type === "password";
    el.apiKey.type = show ? "text" : "password";
    el.keyEye.setAttribute("aria-pressed", String(show));
    el.keyEye.setAttribute("aria-label", show ? "Скрыть ключ" : "Показать ключ");
    el.keyEye.replaceChildren(icon(show ? "i-eye-off" : "i-eye"));
  });

  const saveKey = async () => {
    const key = el.apiKey.value.trim();
    if (!key || settings.provider === "local") return;
    try {
      await invoke("set_api_key", { provider: settings.provider, key });
      ui.keys[settings.provider] = true;
      renderProvider();
    } catch (e) {
      complain("Ключ не сохранился.", e);
    }
  };
  el.apiKey.addEventListener("change", saveKey);
  el.apiKey.addEventListener("keydown", (e) => {
    if (e.key === "Enter") el.apiKey.blur();
  });

  const cloud = () => (settings.provider === "local" ? null : settings[settings.provider]);
  el.model.addEventListener("input", () => {
    const cfg = cloud();
    if (!cfg) return;
    cfg.model = el.model.value.trim();
    scheduleSave();
  });
  el.baseUrl.addEventListener("input", () => {
    const cfg = cloud();
    if (!cfg) return;
    cfg.baseUrl = el.baseUrl.value.trim();
    renderNotes();
    scheduleSave();
  });
  effortSelect = select(
    el.effort,
    EFFORTS,
    () => settings.effort,
    (value) => {
      settings.effort = value;
      scheduleSave(0);
    },
  );
  comboBox(el.model, () =>
    settings.provider === "local" ? [] : MODELS[settings.provider].map((m) => ({ value: m.value, hint: m.label })),
  );
  comboBox(el.targetLang, () => LANGS);
  comboBox(el.secondaryLang, () => LANGS);

  el.test.addEventListener("click", async () => {
    if (el.test.getAttribute("aria-busy") === "true") return;
    await save();
    el.test.setAttribute("aria-busy", "true");
    el.test.textContent = "Жду ответ…";
    el.testBefore.textContent = SAMPLE_RU;
    el.testAfter.className = "preview-after muted";
    el.testAfter.textContent = "Нейронка думает…";
    el.testResult.hidden = false;
    try {
      const answer = await invoke<string>("test_connection", { text: SAMPLE_RU });
      el.testAfter.className = "preview-after swap";
      el.testAfter.textContent = answer;
    } catch (e) {
      el.testAfter.className = "preview-after error";
      el.testAfter.replaceChildren(...squiggleLead(String(e)));
    } finally {
      el.test.removeAttribute("aria-busy");
      el.test.textContent = "Проверить";
    }
  });

  const langChanged = () => {
    settings.targetLang = el.targetLang.value;
    settings.secondaryLang = el.secondaryLang.value;
    refreshActionMeta();
    scheduleSave();
  };
  el.targetLang.addEventListener("input", langChanged);
  el.secondaryLang.addEventListener("input", langChanged);

  el.addAction.addEventListener("click", () => {
    const action: Action = {
      id: `custom-${Date.now().toString(36)}`,
      name: "Своё действие",
      hotkey: "",
      prompt: "Make the text more polite and formal. Keep the original language and meaning.",
      enabled: true,
    };
    settings.actions.push(action);
    expanded = action.id;
    previewId = action.id;
    renderActions();
    el.actions.querySelector<HTMLInputElement>(".action-editor input")?.select();
    scheduleSave(0);
  });

  el.showHud.addEventListener("change", () => {
    settings.showHud = el.showHud.checked;
    scheduleSave(0);
  });

  el.gpu.addEventListener("change", () => {
    settings.local.gpu = el.gpu.checked;
    scheduleSave(0);
  });

  el.autostart.addEventListener("change", async () => {
    try {
      await invoke("set_autostart", { enabled: el.autostart.checked });
    } catch (e) {
      el.autostart.checked = !el.autostart.checked;
      complain("Автозапуск не переключился.", e);
    }
  });

  el.axOpen2.addEventListener("click", openAccessibility);
}

/** Picks up changes made outside this window: tray pause, a granted permission, a new key. */
async function refreshExternal() {
  const fresh = await invoke<UiState>("get_state");
  ui.keys = fresh.keys;
  ui.accessibility = fresh.accessibility;
  settings.paused = fresh.settings.paused;
  renderStatus();
}

async function init() {
  ui = await invoke<UiState>("get_state");
  settings = ui.settings;
  local = await invoke<LocalStatus>("local_status");
  document.documentElement.dataset.platform = ui.platform;
  document.documentElement.dataset.corners = ui.rounded ? "round" : "square";

  el.version.textContent = ui.version;
  el.targetLang.value = settings.targetLang;
  el.secondaryLang.value = settings.secondaryLang;
  el.showHud.checked = settings.showHud;
  el.autostart.checked = ui.autostart;

  renderProvider();
  renderActions();
  bind();
  showView((requiresKey() && !hasKey()) || (settings.provider === "local" && !localReady()) ? "model" : "actions");

  await listen<LocalProgress>("local-progress", ({ payload }) => {
    const finished = payload.stage === "done" || payload.stage === "error" || payload.stage === "cancelled";
    local.download = finished ? null : payload;
    localError = payload.stage === "error" ? { model: payload.model, message: payload.message ?? "Не скачалось" } : null;
    if (finished) {
      // The app picks a model that just came in.
      if (payload.stage === "done") settings.local.model = payload.model;
      void refreshLocal();
    } else if (settings.provider === "local") {
      renderLocal();
    }
  });

  await listen<Settings>("settings-changed", ({ payload }) => {
    if (payload.paused !== settings.paused) {
      settings.paused = payload.paused;
      renderStatus();
    }
  });
  await getCurrentWindow().onFocusChanged(({ payload: focused }) => {
    if (focused) void refreshExternal();
  });
  window.setInterval(() => {
    if (ui.platform === "macos" && !ui.accessibility) void refreshExternal();
  }, 2000);
}

void init();
