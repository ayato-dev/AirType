import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { hotkeyLabel, hotkeyParts, isMac, recordKey } from "./hotkey";

type Provider = "anthropic" | "openai";
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

interface Settings {
  provider: Provider;
  anthropic: ProviderConfig;
  openai: ProviderConfig;
  effort: string;
  targetLang: string;
  secondaryLang: string;
  showHud: boolean;
  paused: boolean;
  actions: Action[];
}

interface HotkeyError {
  actionId: string;
  message: string;
}

interface UiState {
  settings: Settings;
  keys: Record<Provider, boolean>;
  hotkeyErrors: HotkeyError[];
  platform: string;
  accessibility: boolean;
  autostart: boolean;
  version: string;
}

const BUILTIN = new Set(["translate", "fix", "improve"]);
const VIEWS: View[] = ["actions", "model", "general"];

const MODELS: Record<Provider, { value: string; label: string }[]> = {
  anthropic: [
    { value: "claude-opus-5-5", label: "Самая умная" },
    { value: "claude-sonnet-5-5", label: "Баланс" },
    { value: "claude-haiku-5-5", label: "Самая быстрая и дешёвая" },
  ],
  openai: [],
};

const PROVIDER_TEXT: Record<Provider, { key: string; model: string; modelHint: string; url: string; urlHint: string }> = {
  anthropic: {
    key: "sk-ant-…",
    model: "claude-opus-5-5",
    modelHint: "Opus 5.5 пишет лучше всех, Haiku 5.5 отвечает быстрее и стоит дешевле.",
    url: "https://api.anthropic.com",
    urlHint: "Пусто значит официальный api.anthropic.com. Для прокси укажи адрес без /v1.",
  },
  openai: {
    key: "Для локальной модели не нужен",
    model: "Имя модели у провайдера",
    modelHint: "Имя модели пиши так, как в документации провайдера.",
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
  saved: $("saved"),
  views: document.querySelector<HTMLElement>(".views")!,
  tabs: document.querySelector<HTMLElement>(".tabs")!,
  tabButtons: [...document.querySelectorAll<HTMLButtonElement>(".tab")],
  axNotice: $("ax-notice"),
  keyNotice: $("key-notice"),
  pausedNotice: $("paused-notice"),
  previewBefore: $("preview-before"),
  previewKeys: $("preview-keys"),
  previewAfter: $("preview-after"),
  actions: $<HTMLUListElement>("actions"),
  addAction: $<HTMLButtonElement>("add-action"),
  targetLang: $<HTMLInputElement>("target-lang"),
  secondaryLang: $<HTMLInputElement>("secondary-lang"),
  providers: [...document.querySelectorAll<HTMLInputElement>('input[name="provider"]')],
  apiKey: $<HTMLInputElement>("api-key"),
  keyEye: $<HTMLButtonElement>("key-eye"),
  keyState: $("key-state"),
  model: $<HTMLInputElement>("model"),
  models: $<HTMLDataListElement>("models"),
  modelHint: $("model-hint"),
  effortField: $("effort-field"),
  effort: $<HTMLSelectElement>("effort"),
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
let expanded: string | null = null;
let previewId: string | null = null;
let recording: { action: Action; chip: HTMLButtonElement } | null = null;

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
  return settings.provider === "anthropic" || settings.openai.baseUrl.trim() === "";
}

// ---------- saving ----------

let saveTimer: number | undefined;
let savedTimer: number | undefined;

function scheduleSave(delay = 350) {
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(save, delay);
}

async function save() {
  window.clearTimeout(saveTimer);
  try {
    ui.hotkeyErrors = await invoke<HotkeyError[]>("save_settings", { settings });
    refreshActionMeta();
    flash("Сохранено", true);
  } catch (e) {
    flash(`Не сохранилось: ${e}`, false);
  }
}

function flash(text: string, ok: boolean) {
  el.saved.replaceChildren(...(ok ? [icon("i-check")] : []), text);
  el.saved.classList.toggle("error", !ok);
  el.saved.classList.add("show");
  window.clearTimeout(savedTimer);
  savedTimer = window.setTimeout(() => el.saved.classList.remove("show"), ok ? 1400 : 5000);
}

// ---------- views ----------

function showView(view: View, focus = false) {
  const index = VIEWS.indexOf(view);
  el.tabs.style.setProperty("--tab", String(index));
  for (const tab of el.tabButtons) {
    const on = tab.dataset.view === view;
    tab.setAttribute("aria-selected", String(on));
    tab.tabIndex = on ? 0 : -1;
    if (on && focus) tab.focus();
  }
  for (const name of VIEWS) $(`view-${name}`).hidden = name !== view;
  el.views.scrollTop = 0;
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

function renderNotices() {
  el.axNotice.hidden = ui.platform !== "macos" || ui.accessibility;
  el.keyNotice.hidden = !requiresKey() || ui.keys[settings.provider];
  el.pausedNotice.hidden = !settings.paused;
  el.enabled.checked = !settings.paused;

  el.axGroup.hidden = ui.platform !== "macos";
  el.axState.textContent = ui.accessibility
    ? "Выдан, Airtype может копировать и вставлять текст"
    : "Нужен, чтобы копировать и вставлять текст за тебя";
  el.axState.classList.toggle("ok", ui.accessibility);
  el.axOpen2.hidden = ui.accessibility;
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
  const cfg = settings[provider];
  const text = PROVIDER_TEXT[provider];
  for (const radio of el.providers) radio.checked = radio.value === provider;

  const hasKey = ui.keys[provider];
  el.apiKey.value = "";
  el.apiKey.placeholder = hasKey ? "Вставь новый, чтобы заменить" : text.key;
  if (hasKey) {
    el.keyState.replaceChildren(icon("i-check"), "Сохранён");
  } else {
    el.keyState.replaceChildren();
  }

  el.model.value = cfg.model;
  el.model.placeholder = text.model;
  el.models.replaceChildren(
    ...MODELS[provider].map((m) => {
      const option = document.createElement("option");
      option.value = m.value;
      option.label = m.label;
      return option;
    }),
  );
  el.modelHint.textContent = text.modelHint;

  el.effortField.hidden = provider !== "anthropic";
  el.effort.value = settings.effort;
  el.baseUrl.value = cfg.baseUrl;
  el.baseUrl.placeholder = text.url;
  el.baseUrlHint.textContent = text.urlHint;
  el.testResult.hidden = true;
  renderNotices();
}

function renderActions() {
  el.actions.replaceChildren(...settings.actions.map(actionRow));
  renderPreview();
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
  for (const action of settings.actions) {
    const desc = el.actions.querySelector<HTMLElement>(`[data-meta="${action.id}"]`);
    if (!desc) continue;
    const meta = actionMeta(action);
    desc.textContent = meta.text;
    desc.classList.toggle("error", meta.error);
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
  desc.classList.toggle("error", meta.error);
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
  toggle.className = "switch";
  toggle.checked = action.enabled;
  toggle.setAttribute("aria-label", `Включить «${action.name}»`);
  toggle.addEventListener("change", () => {
    action.enabled = toggle.checked;
    li.classList.toggle("off", !action.enabled);
    scheduleSave(0);
    refreshActionMeta();
  });

  head.append(main, chip, toggle);
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
    remove.append(icon("i-trash"), "Удалить");
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

function startRecording(action: Action, chip: HTMLButtonElement) {
  if (recording) stopRecording();
  recording = { action, chip };
  chip.classList.add("recording");
  fillChip(chip, "", "Жми сочетание");
  // The current hotkeys would otherwise fire while the user presses them here.
  void invoke("suspend_hotkeys", { suspended: true });
}

function stopRecording() {
  if (!recording) return;
  const { action, chip } = recording;
  recording = null;
  chip.classList.remove("recording");
  fillChip(chip, action.hotkey);
  void save().then(async () => {
    ui.hotkeyErrors = await invoke<HotkeyError[]>("suspend_hotkeys", { suspended: false });
    refreshActionMeta();
  });
}

window.addEventListener(
  "keydown",
  (e) => {
    if (!recording) {
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
    const { action, chip } = recording;
    switch (result.kind) {
      case "cancel":
        stopRecording();
        break;
      case "clear":
        action.hotkey = "";
        stopRecording();
        break;
      case "pending":
        chip.replaceChildren(combo(result.parts.join(isMac ? "" : "+")));
        break;
      case "invalid":
        fillChip(chip, "", isMac ? "Нужен ⌘, ⌃ или ⌥" : "Нужен Ctrl или Alt");
        break;
      case "done":
        action.hotkey = result.accelerator;
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
  renderNotices();
  void save();
}

function bind() {
  $("close").addEventListener("click", () => void invoke("hide_settings"));
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

  $("key-go").addEventListener("click", () => {
    showView("model");
    el.apiKey.focus();
  });
  $("resume").addEventListener("click", () => setPaused(false));
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
    if (!key) return;
    try {
      await invoke("set_api_key", { provider: settings.provider, key });
      ui.keys[settings.provider] = true;
      renderProvider();
      flash("Ключ сохранён", true);
    } catch (e) {
      flash(`Ключ не сохранился: ${e}`, false);
    }
  };
  el.apiKey.addEventListener("change", saveKey);
  el.apiKey.addEventListener("keydown", (e) => {
    if (e.key === "Enter") el.apiKey.blur();
  });

  el.model.addEventListener("input", () => {
    settings[settings.provider].model = el.model.value.trim();
    scheduleSave();
  });
  el.baseUrl.addEventListener("input", () => {
    settings[settings.provider].baseUrl = el.baseUrl.value.trim();
    renderNotices();
    scheduleSave();
  });
  el.effort.addEventListener("change", () => {
    settings.effort = el.effort.value;
    scheduleSave(0);
  });

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
      el.testAfter.textContent = String(e);
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

  el.autostart.addEventListener("change", async () => {
    try {
      await invoke("set_autostart", { enabled: el.autostart.checked });
      flash("Сохранено", true);
    } catch (e) {
      el.autostart.checked = !el.autostart.checked;
      flash(`Не вышло: ${e}`, false);
    }
  });

  const openAccessibility = () => void invoke("request_accessibility");
  $("ax-open").addEventListener("click", openAccessibility);
  el.axOpen2.addEventListener("click", openAccessibility);
}

/** Picks up changes made outside this window: tray pause, a granted permission, a new key. */
async function refreshExternal() {
  const fresh = await invoke<UiState>("get_state");
  ui.keys = fresh.keys;
  ui.accessibility = fresh.accessibility;
  settings.paused = fresh.settings.paused;
  renderNotices();
}

async function init() {
  ui = await invoke<UiState>("get_state");
  settings = ui.settings;
  document.documentElement.dataset.platform = ui.platform;

  el.version.textContent = ui.version;
  el.targetLang.value = settings.targetLang;
  el.secondaryLang.value = settings.secondaryLang;
  el.showHud.checked = settings.showHud;
  el.autostart.checked = ui.autostart;

  renderProvider();
  renderActions();
  bind();
  showView(requiresKey() && !ui.keys[settings.provider] ? "model" : "actions");

  await listen<Settings>("settings-changed", ({ payload }) => {
    if (payload.paused !== settings.paused) {
      settings.paused = payload.paused;
      renderNotices();
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
