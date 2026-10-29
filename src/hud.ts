import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { squiggleLead } from "./flag";
import { glass } from "./glass";

type HudEvent =
  | { state: "working"; action: string; label: string }
  | { state: "done" }
  | { state: "error"; message: string };

const WORKING: Record<string, string> = {
  translate: "Перевожу",
  fix: "Исправляю",
  improve: "Улучшаю",
};

const slip = document.getElementById("slip") as HTMLElement;
const text = document.getElementById("text") as HTMLElement;

/** On Windows and macOS the window itself is the glass; Linux gets a paper slip with room for its shadow. */
const margin = glass() === "sheet" ? 12 : 0;

/** The marker layer repeats the text so the letters it covers switch to marker ink. */
function say(words: string) {
  text.textContent = words;
  text.dataset.text = words;
}

function sayError(message: string) {
  text.replaceChildren(...squiggleLead(message));
  delete text.dataset.text;
}

function replayEntrance() {
  slip.classList.remove("enter");
  void slip.offsetWidth; // restart the animation
  slip.classList.add("enter");
}

/** The window takes the slip's size; the app then puts it under the text and shows it. */
async function fit() {
  await document.fonts.ready;
  const box = slip.getBoundingClientRect();
  await invoke("hud_fit", {
    width: Math.ceil(box.width) + margin * 2,
    height: Math.ceil(box.height) + margin * 2,
  });
}

void listen<HudEvent>("hud", ({ payload }) => {
  slip.dataset.state = payload.state;
  switch (payload.state) {
    case "working":
      say(`${WORKING[payload.action] ?? payload.label}…`);
      replayEntrance();
      break;
    case "done":
      say("Готово");
      break;
    case "error":
      sayError(payload.message);
      replayEntrance();
      break;
  }
  void fit();
});
