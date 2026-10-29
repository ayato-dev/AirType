// Drop-down lists drawn by the page itself. The system ones (a <select> menu, the
// <datalist> under an input) look like the operating system, not like Airtype,
// and a datalist shows only the entries that match what is already typed.

export interface Choice {
  value: string;
  /** Shown instead of the value; a select shows only this. */
  label?: string;
  /** A note on the right, in muted ink. */
  hint?: string;
}

interface Owner {
  anchor: HTMLElement;
  choices: Choice[];
  current: string;
  pick: (choice: Choice) => void;
}

const menu = document.createElement("div");
menu.className = "menu";
menu.id = "menu";
menu.setAttribute("role", "listbox");
menu.hidden = true;
// Clicking an entry must not take focus from the input it belongs to.
menu.addEventListener("mousedown", (e) => e.preventDefault());
document.body.append(menu);

let owner: Owner | null = null;
let active = -1;
let uid = 0;

export function menuOpen(): boolean {
  return owner !== null;
}

function close() {
  if (!owner) return;
  owner.anchor.setAttribute("aria-expanded", "false");
  owner.anchor.removeAttribute("aria-activedescendant");
  owner = null;
  active = -1;
  menu.hidden = true;
}

function setActive(index: number) {
  const items = [...menu.children] as HTMLElement[];
  if (!owner || items.length === 0) return;
  active = (index + items.length) % items.length;
  items.forEach((item, i) => item.classList.toggle("active", i === active));
  const item = items[active];
  owner.anchor.setAttribute("aria-activedescendant", item.id);
  item.scrollIntoView({ block: "nearest" });
}

function place(anchor: HTMLElement) {
  const box = anchor.getBoundingClientRect();
  const gap = 6;
  const below = window.innerHeight - box.bottom - gap - 8;
  const above = box.top - gap - 8;
  menu.style.left = `${box.left}px`;
  menu.style.minWidth = `${box.width}px`;
  menu.style.maxWidth = `${Math.max(box.width, window.innerWidth - box.left - 12)}px`;
  menu.style.maxHeight = "";
  const natural = Math.min(menu.scrollHeight, 288);
  const down = natural <= below || below >= above;
  const room = Math.max(96, down ? below : above);
  menu.style.maxHeight = `${Math.min(288, room)}px`;
  const height = Math.min(natural, room);
  menu.style.top = `${down ? box.bottom + gap : box.top - gap - height}px`;
  menu.dataset.side = down ? "below" : "above";
}

function open(next: Owner, choices: Choice[]) {
  if (choices.length === 0) {
    close();
    return;
  }
  if (owner && owner.anchor !== next.anchor) close();
  owner = next;
  next.anchor.setAttribute("aria-expanded", "true");
  next.anchor.setAttribute("aria-controls", menu.id);
  menu.replaceChildren(
    ...choices.map((choice, i) => {
      const item = document.createElement("div");
      item.className = "menu-item";
      item.id = `menu-${++uid}`;
      item.setAttribute("role", "option");
      const chosen = choice.value === next.current;
      item.setAttribute("aria-selected", String(chosen));
      const text = document.createElement("span");
      text.className = "menu-text";
      text.textContent = choice.label ?? choice.value;
      item.append(text);
      if (choice.hint) {
        const hint = document.createElement("span");
        hint.className = "menu-hint";
        hint.textContent = choice.hint;
        item.append(hint);
      }
      item.addEventListener("mousemove", () => {
        if (active !== i) setActive(i);
      });
      item.addEventListener("click", () => {
        next.pick(choice);
        close();
      });
      return item;
    }),
  );
  menu.hidden = false;
  place(next.anchor);
  const chosen = choices.findIndex((c) => c.value === next.current);
  if (chosen >= 0) setActive(chosen);
  else active = -1;
}

/** Arrow keys, Enter and Escape for an open list; true when the key was used. */
function navigate(e: KeyboardEvent): boolean {
  if (!owner) return false;
  switch (e.key) {
    case "ArrowDown":
      setActive(active + 1);
      return true;
    case "ArrowUp":
      setActive(active < 0 ? -1 : active - 1);
      return true;
    case "Enter": {
      const item = menu.children[active] as HTMLElement | undefined;
      if (!item) return false;
      item.click();
      return true;
    }
    case "Escape":
      close();
      return true;
    case "Tab":
      close();
      return false;
  }
  return false;
}

/**
 * A text input with a list under it. Opening it shows every entry; typing
 * narrows the list, and anything not in it can still be typed.
 */
export function combo(input: HTMLInputElement, choices: () => Choice[]) {
  input.setAttribute("role", "combobox");
  input.setAttribute("aria-autocomplete", "list");
  input.setAttribute("aria-expanded", "false");
  input.autocomplete = "off";

  const self = (): Owner => ({
    anchor: input,
    choices: choices(),
    current: input.value.trim(),
    pick: (choice) => {
      input.value = choice.value;
      input.dispatchEvent(new Event("input", { bubbles: true }));
      input.dispatchEvent(new Event("change", { bubbles: true }));
    },
  });

  const showAll = () => {
    const o = self();
    open(o, o.choices);
  };

  input.addEventListener("focus", showAll);
  input.addEventListener("click", () => {
    if (owner?.anchor !== input) showAll();
  });
  input.addEventListener("input", (e) => {
    // Our own pick fires input too; the list is closed by then.
    if (!e.isTrusted) return;
    const o = self();
    const query = input.value.trim().toLowerCase();
    const exact = o.choices.some((c) => c.value.toLowerCase() === query);
    const shown =
      query === "" || exact
        ? o.choices
        : o.choices.filter((c) =>
            [c.value, c.label ?? "", c.hint ?? ""].some((s) => s.toLowerCase().includes(query)),
          );
    open(o, shown);
  });
  input.addEventListener("keydown", (e) => {
    if (!owner && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      showAll();
      e.preventDefault();
      return;
    }
    if (navigate(e)) e.preventDefault();
  });
  input.addEventListener("blur", close);
}

/** A button that opens a list of fixed choices, like a <select>. */
export function select(
  button: HTMLButtonElement,
  choices: Choice[],
  get: () => string,
  set: (value: string) => void,
) {
  button.setAttribute("aria-haspopup", "listbox");
  button.setAttribute("aria-expanded", "false");
  const text = document.createElement("span");
  text.className = "select-text";
  button.replaceChildren(text);

  const render = () => {
    const value = get();
    const choice = choices.find((c) => c.value === value) ?? choices[0];
    text.textContent = choice.label ?? choice.value;
  };

  const self = (): Owner => ({
    anchor: button,
    choices,
    current: get(),
    pick: (choice) => {
      set(choice.value);
      render();
    },
  });

  button.addEventListener("click", () => {
    if (owner?.anchor === button) close();
    else open(self(), choices);
  });
  button.addEventListener("keydown", (e) => {
    if (!owner && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      open(self(), choices);
      e.preventDefault();
      return;
    }
    if (owner?.anchor === button && e.key === " ") {
      (menu.children[active] as HTMLElement | undefined)?.click();
      e.preventDefault();
      return;
    }
    if (navigate(e)) e.preventDefault();
  });
  button.addEventListener("blur", close);
  render();
  return { render };
}

// The list stays put while the page under it moves; close it instead of chasing it.
document.addEventListener(
  "scroll",
  (e) => {
    if (e.target !== menu) close();
  },
  true,
);
window.addEventListener("resize", close);
window.addEventListener("blur", close);
