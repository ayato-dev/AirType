import { listen } from "@tauri-apps/api/event";

type HudEvent =
  | { state: "working"; action: string; label: string }
  | { state: "done" }
  | { state: "error"; message: string };

const WORKING: Record<string, string> = {
  translate: "Перевожу",
  fix: "Исправляю",
  improve: "Улучшаю",
};

const pill = document.getElementById("pill") as HTMLElement;
const text = document.getElementById("text") as HTMLElement;

function replayEntrance() {
  pill.classList.remove("enter");
  void pill.offsetWidth; // restart the animation
  pill.classList.add("enter");
}

void listen<HudEvent>("hud", ({ payload }) => {
  pill.dataset.state = payload.state;
  switch (payload.state) {
    case "working":
      text.textContent = `${WORKING[payload.action] ?? payload.label}…`;
      replayEntrance();
      break;
    case "done":
      text.textContent = "Готово";
      break;
    case "error":
      text.textContent = payload.message;
      replayEntrance();
      break;
  }
});
