import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { hotkeyParts, isMac, recordKey } from "./hotkey";

type Provider = "anthropic" | "openai";

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
const ICONS: Record<string, string> = { translate: "⇄", fix: "✓", improve: "✦" };

const MODELS: Record<Provider, { value: string; label: string }[]> = {
  anthropic: [
    { value: "claude-opus-5-5", label: "самая умная" },
    { value: "claude-sonnet-5-5", label: "баланс" },
    { value: "claude-haiku-5-5", label: "самая быстрая и дешёвая" },
  ],
  openai: [],
};

const PROVIDER_TEXT: Record<Provider, { key: string; model: string; url: string; urlHint: string }> = {
  anthropic: {
    key: "sk-ant-…",
    model: "claude-opus-5-5",
    url: "https://api.anthropic.com",
    urlHint: "Пусто — официальный api.anthropic.com. Для прокси укажи адрес без /v1.",
  },
  openai: {
    key: "sk-…  (для локальной модели не нужен)",
    model: "имя модели у провайдера",
    url: "https://api.openai.com/v1",
    urlHint: "Пусто — api.openai.com/v1. OpenRouter: https://openrouter.ai/api/v1 · Ollama: http://localhost:11434/v1",
  },
};

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const el = {
  status: $<HTMLButtonElement>("status"),
  statusText: $("status-text"),
  heroKeys: $("hero-keys"),
  axBanner: $("ax-banner"),
  provider: $("provider"),
  apiKey: $<HTMLInputElement>("api-key"),
  keyEye: $<HTMLButtonElement>("key-eye"),
  keyState: $("key-state"),
  model: $<HTMLInputElement>("model"),
  models: $<HTMLDataListElement>("models"),
  effortRow: $("effort-row"),
  effort: $<HTMLSelectElement>("effort"),
  baseUrl: $<HTMLInputElement>("base-url"),
  baseUrlHint: $("base-url-hint"),
  test: $<HTMLButtonElement>("test"),
  testResult: $("test-result"),
  targetLang: $<HTMLInputElement>("target-lang"),
  secondaryLang: $<HTMLInputElement>("secondary-lang"),
  actions: $<HTMLUListElement>("actions"),
  addAction: $<HTMLButtonElement>("add-action"),
  showHud: $<HTMLInputElement>("show-hud"),
  autostart: $<HTMLInputElement>("autostart"),
  saved: $("saved"),
  version: $("version"),
};

let ui: UiState;
let settings: Settings;
let expanded: string | null = null;
let recording: { action: Action; chip: HTMLButtonElement } | null = null;

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
  el.saved.textContent = text;
  el.saved.classList.toggle("flash", ok);
  window.clearTimeout(savedTimer);
  savedTimer = window.setTimeout(() => {
    el.saved.textContent = "Сохраняется само";
    el.saved.classList.remove("flash");
  }, 1600);
}

// ---------- rendering ----------

function keycaps(accelerator: string): HTMLElement[] {
  return hotkeyParts(accelerator).map((part) => {
    const cap = document.createElement("span");
    cap.className = "kbd";
    cap.textContent = part;
    return cap;
  });
}

function renderStatus() {
  el.status.classList.toggle("paused", settings.paused);
  el.statusText.textContent = settings.paused ? "на паузе" : "работает";
}

function renderHero() {
  const main = settings.actions.find((a) => a.enabled && a.hotkey);
  if (main) {
    el.heroKeys.replaceChildren(...keycaps(main.hotkey));
  } else {
    const note = document.createElement("span");
    note.className = "hero-sub";
    note.textContent = "назначь сочетание в «Действиях»";
    el.heroKeys.replaceChildren(note);
  }
}

function renderProvider() {
  const provider = settings.provider;
  const cfg = settings[provider];
  const text = PROVIDER_TEXT[provider];
  el.provider.dataset.index = provider === "anthropic" ? "0" : "1";
  el.provider.querySelectorAll<HTMLButtonElement>("button").forEach((b) => {
    b.classList.toggle("active", b.dataset.value === provider);
  });

  const hasKey = ui.keys[provider];
  el.apiKey.value = "";
  el.apiKey.placeholder = hasKey ? "сохранён · вставь новый, чтобы заменить" : text.key;
  el.keyState.textContent = hasKey ? "✓ в системной связке ключей" : "";
  el.keyState.classList.toggle("ok", hasKey);

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

  el.effortRow.hidden = provider !== "anthropic";
  el.effort.value = settings.effort;
  el.baseUrl.value = cfg.baseUrl;
  el.baseUrl.placeholder = text.url;
  el.baseUrlHint.textContent = text.urlHint;
  el.testResult.hidden = true;
}

function renderActions() {
  el.actions.replaceChildren(...settings.actions.map(actionRow));
  renderHero();
}

function actionMeta(action: Action): { text: string; error: boolean } {
  const error = ui.hotkeyErrors.find((e) => e.actionId === action.id);
  if (error && action.enabled) return { text: error.message, error: true };
  if (!action.enabled) return { text: "выключено", error: false };
  return { text: action.hotkey ? "нажми, чтобы настроить" : "без сочетания", error: false };
}

function refreshActionMeta() {
  for (const action of settings.actions) {
    const small = el.actions.querySelector<HTMLElement>(`[data-meta="${action.id}"]`);
    if (!small) continue;
    const meta = actionMeta(action);
    small.textContent = meta.text;
    small.classList.toggle("error", meta.error);
  }
  renderHero();
}

function actionRow(action: Action): HTMLLIElement {
  const li = document.createElement("li");
  li.className = "action";
  li.classList.toggle("disabled", !action.enabled);

  const head = document.createElement("div");
  head.className = "action-head";

  const icon = document.createElement("span");
  icon.className = "action-icon";
  icon.textContent = ICONS[action.id] ?? "✧";

  const name = document.createElement("button");
  name.type = "button";
  name.className = "action-name";
  const title = document.createElement("strong");
  title.textContent = action.name || "Без названия";
  const small = document.createElement("small");
  small.dataset.meta = action.id;
  const meta = actionMeta(action);
  small.textContent = meta.text;
  small.classList.toggle("error", meta.error);
  name.append(title, small);
  name.addEventListener("click", () => {
    expanded = expanded === action.id ? null : action.id;
    renderActions();
  });

  const chip = document.createElement("button");
  chip.type = "button";
  chip.className = "hotkey";
  chip.title = "Нажми и введи новое сочетание";
  fillChip(chip, action.hotkey);
  chip.addEventListener("click", () => startRecording(action, chip));

  const toggle = document.createElement("input");
  toggle.type = "checkbox";
  toggle.className = "switch";
  toggle.checked = action.enabled;
  toggle.setAttribute("aria-label", `Включить «${action.name}»`);
  toggle.addEventListener("change", () => {
    action.enabled = toggle.checked;
    li.classList.toggle("disabled", !action.enabled);
    scheduleSave(0);
    refreshActionMeta();
  });

  head.append(icon, name, chip, toggle);
  li.append(head);

  if (expanded === action.id) li.append(actionEditor(action, title));
  return li;
}

function actionEditor(action: Action, title: HTMLElement): HTMLElement {
  const body = document.createElement("div");
  body.className = "action-body";

  const nameInput = document.createElement("input");
  nameInput.value = action.name;
  nameInput.placeholder = "Название";
  nameInput.addEventListener("input", () => {
    action.name = nameInput.value;
    title.textContent = action.name || "Без названия";
    scheduleSave();
  });

  const prompt = document.createElement("textarea");
  prompt.value = action.prompt;
  prompt.placeholder = "Что сделать с текстом. Например: «Сделай вежливее и короче».";
  prompt.spellcheck = false;
  prompt.addEventListener("input", () => {
    action.prompt = prompt.value;
    scheduleSave();
  });

  const tools = document.createElement("div");
  tools.className = "action-tools";
  const hint = document.createElement("span");
  hint.textContent = "{target} и {secondary} — языки из «Перевода»";
  tools.append(hint);

  if (!BUILTIN.has(action.id)) {
    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "link-danger";
    remove.textContent = "Удалить";
    remove.addEventListener("click", () => {
      settings.actions = settings.actions.filter((a) => a !== action);
      expanded = null;
      renderActions();
      scheduleSave(0);
    });
    tools.append(remove);
  }

  body.append(nameInput, prompt, tools);
  return body;
}

function fillChip(chip: HTMLButtonElement, accelerator: string, placeholder = "задать") {
  if (accelerator) {
    chip.replaceChildren(...keycaps(accelerator));
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
  fillChip(chip, "", "жми сочетание…");
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
        chip.replaceChildren(...result.parts.map((p) => Object.assign(document.createElement("span"), { className: "kbd", textContent: p })));
        break;
      case "invalid":
        fillChip(chip, "", isMac ? "нужен ⌘, ⌃ или ⌥" : "нужен Ctrl или Alt");
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

function bind() {
  el.status.addEventListener("click", () => {
    settings.paused = !settings.paused;
    renderStatus();
    void save();
  });

  $("close").addEventListener("click", () => void invoke("hide_settings"));

  el.provider.querySelectorAll<HTMLButtonElement>("button").forEach((button) => {
    button.addEventListener("click", () => {
      settings.provider = button.dataset.value as Provider;
      renderProvider();
      scheduleSave(0);
    });
  });

  el.keyEye.addEventListener("click", () => {
    const show = el.apiKey.type === "password";
    el.apiKey.type = show ? "text" : "password";
    el.keyEye.classList.toggle("on", show);
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
    scheduleSave();
  });
  el.effort.addEventListener("change", () => {
    settings.effort = el.effort.value;
    scheduleSave(0);
  });

  el.test.addEventListener("click", async () => {
    await save();
    el.test.disabled = true;
    el.test.textContent = "Думаю…";
    el.testResult.hidden = true;
    try {
      const answer = await invoke<string>("test_connection");
      el.testResult.textContent = `Работает: ${answer}`;
      el.testResult.classList.remove("error");
    } catch (e) {
      const message = String(e);
      el.testResult.textContent = message.charAt(0).toUpperCase() + message.slice(1);
      el.testResult.classList.add("error");
    } finally {
      el.testResult.hidden = false;
      el.test.disabled = false;
      el.test.textContent = "Проверить";
    }
  });

  el.targetLang.addEventListener("input", () => {
    settings.targetLang = el.targetLang.value;
    scheduleSave();
  });
  el.secondaryLang.addEventListener("input", () => {
    settings.secondaryLang = el.secondaryLang.value;
    scheduleSave();
  });

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
    renderActions();
    el.actions.querySelector<HTMLInputElement>(".action-body input")?.select();
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

  $("ax-open").addEventListener("click", () => void invoke("request_accessibility"));
}

/** Picks up changes made outside this window: tray pause, a granted permission, a new key. */
async function refreshExternal() {
  const fresh = await invoke<UiState>("get_state");
  ui.keys = fresh.keys;
  ui.accessibility = fresh.accessibility;
  el.axBanner.hidden = fresh.platform !== "macos" || fresh.accessibility;
  if (fresh.settings.paused !== settings.paused) {
    settings.paused = fresh.settings.paused;
    renderStatus();
  }
}

async function init() {
  ui = await invoke<UiState>("get_state");
  settings = ui.settings;
  document.documentElement.dataset.platform = ui.platform;

  el.version.textContent = `v${ui.version}`;
  el.targetLang.value = settings.targetLang;
  el.secondaryLang.value = settings.secondaryLang;
  el.showHud.checked = settings.showHud;
  el.autostart.checked = ui.autostart;
  el.axBanner.hidden = ui.platform !== "macos" || ui.accessibility;

  renderStatus();
  renderProvider();
  renderActions();
  bind();

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
    if (!el.axBanner.hidden) void refreshExternal();
  }, 2000);
}

void init();
