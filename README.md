<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="92" alt="" />
</p>

<h1 align="center">Airtype</h1>

<p align="center">
  Translator and text editor for any input field.
</p>

<p align="center">
  <a href="https://github.com/ayato-dev/AirType/releases/latest">Download</a> for Windows, macOS and Linux
</p>

<p align="center">
  English | <a href="README.ru.md">Русский</a>
</p>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/media/translate-dark.webp" />
  <img src="docs/media/translate-light.webp" alt="A message typed in Russian in a messenger. After Ctrl+Alt+T a status label appears under the field, and a second later the field holds the same text in English." />
</picture>

Airtype runs in the tray and works in messengers, browsers, mail clients, editors and other apps with an input field. It reads the text from the field, sends it to the model you chose and writes the answer back. `Ctrl+Z` undoes the replacement.

## Actions

### Translate

`Ctrl+Alt+T` on Windows and Linux, `⌘⌥T` on macOS. Russian text becomes English and English text becomes Russian. You can pick other languages in the settings or type any language name.

### Fix mistakes

`Ctrl+Alt+R` or `⌘⌥R`. Corrects typos, punctuation and spelling and leaves the rest of the text as it is.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/media/fix-dark.webp" />
  <img src="docs/media/fix-light.webp" alt="An email with underlined mistakes. After Ctrl+Alt+R the commas are in place, the typo is fixed and the underlines are gone." />
</picture>

### Rewrite

`Ctrl+Alt+E` or `⌘⌥E`. Makes a hastily typed message easier to read. The meaning, language and approximate length stay the same.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/media/improve-dark.webp" />
  <img src="docs/media/improve-light.webp" alt="A rough chat message in Russian. After Ctrl+Alt+E it becomes a clearer version of the same message and is sent to the chat." />
</picture>

### Write for me

`Ctrl+Alt+Space` or `⌘⌥A` in an empty field opens a panel under the field. Describe what to write, then choose a language and tone. The panel shows the result, and its `Вставить` (Insert) button puts the text in the field.

Esc, a click outside the panel or switching to another window closes it. Closing it while the model is working cancels the request.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/media/compose-dark.webp" />
  <img src="docs/media/compose-light.webp" alt="A panel opens over an empty field. The request says to decline Friday and suggest Monday afternoon, with a friendly tone selected. The panel writes a polite reply, and the reply goes into the field." />
</picture>

### Work with selected text

Select text anywhere, including pages where it can't be edited, and press `Ctrl+Alt+Space` or `⌘⌥A`. The panel offers translate, fix, improve, your own actions and a field for a one-off instruction.

The panel opens under the selection. If the app doesn't report where the selection is (Telegram and many other chat apps), it opens under the mouse pointer. You can copy the answer, and if the text was in an input field you can replace the selection with it.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/media/read-dark.webp" />
  <img src="docs/media/read-light.webp" alt="A paragraph about opening hours is selected on an English museum page. The panel under it translates the paragraph into Russian, and the answer is copied." />
</picture>

### Custom actions and models

You can add an action with its own instruction and hotkey in the settings, for example "make it more polite and shorter". Hotkeys, languages and the model are set there too. The available models are Claude, any service with an OpenAI-compatible API, and Qwen running on your computer with no key and no internet connection.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/media/settings-dark.webp" />
  <img src="docs/media/settings-light.webp" alt="Two settings windows. The left one lists actions with their hotkeys: translate, fix, improve and a custom action. The right one has the local model selected, with Qwen3.5 2B downloaded and 4B downloading." />
</picture>

## Install

Installers are in the [latest release](https://github.com/ayato-dev/AirType/releases/latest).

| System | File |
|---|---|
| Windows 10 and 11 | `Airtype_…_x64-setup.exe` (no admin rights needed) or `Airtype_…_x64_en-US.msi` |
| macOS 11 or newer, Intel and Apple Silicon | `Airtype_…_universal.dmg` |
| Linux on X11, Ubuntu 22.04 or newer, Debian | `Airtype_…_amd64.deb` |
| Linux on X11, any distribution | `Airtype_…_amd64.AppImage` |

After installing, open the settings from the tray icon, then paste an API key or download a local model.

The builds are not code signed yet, so the system shows a warning on first launch.

### Windows

SmartScreen reports an unknown publisher. Click "More info", then "Run anyway".

### macOS

Drag Airtype to Applications and run this once in Terminal:

```sh
xattr -cr /Applications/Airtype.app
```

Then allow Airtype in System Settings > Privacy & Security > Accessibility. It needs this to press `⌘C` and `⌘V`. The current ad-hoc signature makes macOS treat each update as a new app, so you have to grant the permission again after updating.

### Linux

Airtype works in X11 sessions only. Hotkeys don't work on Wayland. Make the AppImage executable before running it: `chmod +x Airtype_*.AppImage`.

### Updates

Since 0.3.0 Airtype checks the latest release at startup and every 12 hours. When a new version is out, an update item appears in the tray and in the settings. It downloads and installs the signed update, then restarts the app.

- Going from 0.2.x to 0.3.0 takes one manual install.
- A `.deb` install doesn't update itself. Install the new `.deb` instead.
- On Windows the update uses the `.exe` installer, even if you installed from the `.msi`.

## Hotkeys

| Windows and Linux | macOS | Action |
|---|---|---|
| `Ctrl+Alt+T` | `⌘⌥T` | Translate into your language (English by default). Text already in that language goes into your second language (Russian by default) |
| `Ctrl+Alt+R` | `⌘⌥R` | Fix spelling, grammar and punctuation |
| `Ctrl+Alt+E` | `⌘⌥E` | Rewrite to read clearer and more naturally |
| `Ctrl+Alt+Space` | `⌘⌥A` | Open the panel: write for me, or actions on the selected text |

With part of a field selected, only that part is processed. With no selection, Airtype takes the whole field the caret is in. While the model is working, a status label stays under the field, or above it if there is no room below. You can change all hotkeys in the settings.

## Models and keys

Airtype uses your own API key. Keys are stored in the system credential store: Keychain on macOS, Credential Manager on Windows, GNOME Keyring or KWallet on Linux.

### Claude

Get a key in the [Anthropic Console](https://platform.claude.com/settings/keys). The default is `claude-haiku-5-5` at low effort, the fastest and cheapest option. Choose `claude-opus-5-5` for better text.

If the Anthropic API isn't available in your country, use a VPN, set your own proxy in `Модель > Свой адрес API` (Model > Custom API address), or use OpenRouter.

### OpenAI-compatible API

OpenAI, OpenRouter (`https://openrouter.ai/api/v1`), and local servers such as Ollama (`http://localhost:11434/v1`) and LM Studio. Local servers don't need a key. A key is sent over `https` only, and plain `http` is allowed only for `localhost`.

### Local model

Qwen3.5 2B (1.3 GB) or 4B (2.7 GB) through [llama.cpp](https://github.com/ggml-org/llama.cpp), downloaded from the settings.

It runs on the GPU when there is one: Vulkan on Windows and Linux (NVIDIA, AMD or Intel with a regular driver), Metal on Apple Silicon. Otherwise it runs on the CPU. If the model doesn't load onto the GPU in time, Airtype restarts it on the CPU. The `Аппаратное ускорение` (Hardware acceleration) switch moves the model to the CPU and frees video memory at once.

A running model takes 2 to 4 GB of video or system memory and unloads after 10 idle minutes.

### What leaves your computer

Text goes only to the API you selected. After a paste the clipboard is restored with all its formats, including images, files and formatted text. Airtype skips password fields.

## Limitations

- The interface is in Russian only.
- In terminals Airtype works on selected text only. On Windows `Ctrl+C` with no selection would interrupt the running program, and `Ctrl+A` would take the whole history. Terminals built into editors, such as VS Code, are not detected as terminals.
- On Linux, terminals use `Ctrl+Shift+C` and `Ctrl+Shift+V`. xterm and urxvt don't work because they have no hotkey that copies without interrupting the program.
- On Windows the clipboard formats that hold GDI handles (metafiles, palettes) are not restored, and an image comes back as `CF_DIB`.
- The accessibility API path hasn't been tested live across a wide range of apps. On macOS, Airtype asks Electron apps to build their accessibility tree for the length of one action and then turns it off.
- Linux works in X11 sessions only, where Airtype catches hotkeys through X and sends `Ctrl+C` and `Ctrl+V` through XTest. Wayland lets apps do neither, so there Airtype works only in XWayland apps.
- On Linux, field text is read through the clipboard only, and only text is restored to the clipboard after a paste.

## Development

You need Rust (stable), Node.js 22 and the [Tauri system dependencies](https://tauri.app/start/prerequisites/).

```sh
npm install
npm run tauri dev     # run with hot reload for the interface
npm run tauri build   # installer in src-tauri/target/release/bundle
cd src-tauri && cargo test
```

### Releases

Bump the version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, then either run `git tag v0.3.0 && git push origin v0.3.0` or run the build workflow from the Actions tab with the release box checked.

GitHub Actions builds the installers on all three systems, signs the updates and publishes the release with `latest.json`, which the app reads to find new versions. Release notes come from `.github/release-notes.md`. Every push to `main` also produces builds, available as artifacts of the build workflow.

Signing needs the `TAURI_SIGNING_PRIVATE_KEY` repository secret, plus `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` if the key has a password. The key must match `plugins.updater.pubkey` in `tauri.conf.json`. A release fails without it, and regular builds produce installers with no updates.

### README images

The scenes in `docs/scenes` render the animations with the app's real styles: the Airtype windows load `src/styles`, and the settings window is the built settings page. To render them again after a UI change, run `node docs/scenes/render.cjs`. This needs Playwright with Chromium and ffmpeg with libwebp.

<details>
<summary>macOS code signing</summary>

Accessibility permission and Keychain entries survive updates only with a Developer ID signature and notarization. To set this up once in your Apple Developer account:

1. Get a "Developer ID Application" certificate.
2. Add it and the notarization credentials to the repository secrets: `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`.
3. Pass them to the `tauri build` step.
4. Remove `"signingIdentity": "-"` from `tauri.conf.json`.

Details: https://v2.tauri.app/distribute/sign/macos/

</details>

<details>
<summary>Architecture</summary>

Rust core on Tauri 2, with an interface in plain TypeScript and no framework.

```
src-tauri/src/
  engine.rs   one run: read the text, call the model, write the answer back
  panel.rs    the panel under the field: "write for me" and actions on a selection
  local.rs    local model: downloads llama.cpp and Qwen, runs the server
  popup.rs    where the label and panel go: under the text, above it or under the mouse
  glass.rs    glass under the windows (acrylic on Windows) and the panel's fade
  field.rs    field text through accessibility APIs (AX on macOS, UI Automation on Windows)
  input.rs    Ctrl/Cmd+C, A, V by physical key code (any keyboard layout; XTest on Linux)
  clipboard.rs clipboard snapshot with all formats, and restoring it
  llm.rs      Claude Messages API, OpenAI-compatible chat/completions, local model
  hotkeys.rs  global hotkeys, fired on key release
  hud.rs      the status label under the field, which never takes focus
  update.rs   updates from releases: check, download, install
  config.rs   settings (JSON in the app folder) and keys (system credential store)
  tray.rs     tray icon: settings, pause, quit
src/          settings window, label and panel (TypeScript + CSS)
design/       icon and tray icon sources
docs/scenes/  scenes for the README animations
```

The settings window uses system glass on macOS (Liquid Glass on macOS 26, a translucent backing on older versions) and acrylic on Windows. The interface borrows from proofreading: the only accent color is a highlighter, the selected item is marked with it, and messages underline the problem with a wavy line. The Ysabeau Office font ships with the app (`src/assets/fonts`, OFL license), so the interface needs no network.

Airtype first asks the field for its text through the accessibility API. This leaves the clipboard alone, shows whether anything is selected and lets Airtype skip password fields. On macOS the answer is also written directly to the field, and on web pages by pasting, so the page registers the change. A short one-line answer is typed as keystrokes.

On Windows a longer answer goes in with `Ctrl+V` so undo works. The clipboard holds only a promise: the text is handed over when the app asks for it, and the previous contents are restored right after. If an app doesn't expose its fields, Airtype presses `Ctrl/Cmd+C` and restores the clipboard before it calls the model. If the text changed while the model was working, or there is nowhere to paste, the answer appears in the panel to be copied.

Temporary clipboard data is marked to stay out of clipboard history (Win+V and clipboard managers on macOS). An answer cut off by the model's limit is not inserted.

The settings window is created when you open it and destroyed when you close it, so no extra web engine runs in the background. If the settings file can't be read, Airtype moves it aside and reports the problem.

</details>

## License

[MIT](LICENSE). The Ysabeau Office font is under the [OFL](src/assets/fonts/OFL.txt).
