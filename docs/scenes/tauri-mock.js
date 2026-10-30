// Stands in for the Rust side when the settings window is drawn for the README:
// answers the few commands the page asks for at start with a believable state.
(() => {
  const action = (id, name, key, prompt, enabled = true) => ({
    id,
    name,
    hotkey: `CommandOrControl+Alt+Key${key}`,
    prompt,
    enabled,
  });

  const settings = {
    provider: "local",
    anthropic: { model: "claude-haiku-5-5", baseUrl: "" },
    openai: { model: "", baseUrl: "" },
    local: { model: "qwen3.5-2b" },
    effort: "low",
    targetLang: "English",
    secondaryLang: "Русский",
    showHud: true,
    paused: false,
    actions: [
      action("translate", "Перевести", "T", ""),
      action("fix", "Исправить", "R", ""),
      action("improve", "Улучшить", "E", ""),
      action("polite", "Вежливее", "P", "Сделай вежливее и короче, смысл не меняй"),
    ],
    panel: { hotkey: "Control+Alt+Space", enabled: true },
  };

  const SMALL = 1_280_835_840;
  const LARGE = 2_740_937_888;
  const answers = {
    get_state: {
      settings,
      keys: { anthropic: true, openai: false },
      hotkeyErrors: [],
      notice: null,
      platform: "windows",
      rounded: true,
      wayland: false,
      localSupported: true,
      accessibility: true,
      autostart: true,
      version: "0.2.1",
    },
    local_status: {
      models: [
        { id: "qwen3.5-2b", name: "Qwen3.5 2B", note: "Быстрая, для перевода и исправлений", size: SMALL, downloaded: true },
        { id: "qwen3.5-4b", name: "Qwen3.5 4B", note: "Точнее и пишет живее, но вдвое медленнее", size: LARGE, downloaded: false },
      ],
      download: { model: "qwen3.5-4b", stage: "model", done: Math.round(LARGE * 0.43), total: LARGE, message: null },
    },
  };

  let next = 1;
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
    transformCallback: () => next++,
    convertFileSrc: (path) => path,
    invoke: async (cmd) => (cmd.startsWith("plugin:event|") ? next++ : structuredClone(answers[cmd] ?? null)),
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
})();
