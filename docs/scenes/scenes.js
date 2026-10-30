// The README animations. Every scene is a function of time: seek(t) draws the frame
// at t seconds from scratch, so render.cjs can step through it frame by frame.
// Airtype's own windows load the app's stylesheets (see frame.html); everything
// around them is someone else's app, drawn plainly.

// ---------- time ----------

const clamp = (x, a = 0, b = 1) => Math.min(b, Math.max(a, x));
const seg = (t, a, b) => clamp((t - a) / (b - a));
const lerp = (a, b, k) => a + (b - a) * k;

function bezier(x1, y1, x2, y2) {
  const cx = 3 * x1;
  const bx = 3 * (x2 - x1) - cx;
  const ax = 1 - cx - bx;
  const cy = 3 * y1;
  const by = 3 * (y2 - y1) - cy;
  const ay = 1 - cy - by;
  const X = (s) => ((ax * s + bx) * s + cx) * s;
  const Y = (s) => ((ay * s + by) * s + cy) * s;
  return (x) => {
    if (x <= 0) return 0;
    if (x >= 1) return 1;
    let lo = 0;
    let hi = 1;
    let s = x;
    for (let i = 0; i < 40; i++) {
      const v = X(s);
      if (Math.abs(v - x) < 1e-6) break;
      if (v < x) lo = s;
      else hi = s;
      s = (lo + hi) / 2;
    }
    return Y(s);
  };
}

/** The app's --ease. */
const ease = bezier(0.2, 0, 0, 1);
/** A hand moving the mouse: starts slowly, lands softly. */
const glide = bezier(0.45, 0, 0.15, 1);

function random(seed) {
  return () => {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let x = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    x = (x + Math.imul(x ^ (x >>> 7), 61 | x)) ^ x;
    return ((x ^ (x >>> 14)) >>> 0) / 4294967296;
  };
}

/** Someone typing `text` from `start`, about `cps` characters a second, a little unevenly. */
function typist(text, start, cps, seed) {
  const rnd = random(seed);
  const times = [];
  let t = start;
  for (const ch of text) {
    t += (1 / cps) * (0.5 + rnd()) + (/[,.!?]/.test(ch) ? 0.14 : 0) + (ch === " " ? 0.02 : 0);
    times.push(t);
  }
  const chars = [...text];
  return {
    start,
    end: t,
    count: (now) => times.filter((x) => x <= now).length,
    at(now) {
      return chars.slice(0, this.count(now)).join("");
    },
    /** When the text last changed, for the caret: it stays solid while you type. */
    last: (now) => times.filter((x) => x <= now).pop() ?? start,
  };
}

const caretOn = (t, since) => t - since < 0.5 || (t - since - 0.5) % 1.06 < 0.53;

/** The HUD's marker sweep (hud.css @keyframes sweep), as [right, left] insets in %. */
function sweep(u) {
  const p = (u % 1.5) / 1.5;
  if (p < 0.55) return [100 * (1 - ease(p / 0.55)), 0];
  return [0, 100 * ease((p - 0.55) / 0.45)];
}

/** "Готово": the marker goes over the whole word from wherever the sweep was. */
function settle(from, k) {
  return [from[0] * (1 - ease(k)), from[1] * (1 - ease(k))];
}

const clip = ([r, l]) => `inset(0 ${r.toFixed(2)}% 0 ${l.toFixed(2)}%)`;

// ---------- building ----------

const stage = document.getElementById("stage");

function h(html) {
  const t = document.createElement("template");
  t.innerHTML = html.trim();
  return t.content.firstElementChild;
}

const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;");

function box(el) {
  const s = stage.getBoundingClientRect();
  const r = el.getBoundingClientRect();
  return { x: r.left - s.left, y: r.top - s.top, w: r.width, h: r.height };
}

const center = (r) => ({ x: r.x + r.w / 2, y: r.y + r.h / 2 });

/** One of Airtype's windows: the app's page in an iframe on a piece of glass. */
class AirWindow {
  constructor(kind) {
    this.kind = kind;
    this.el = h(`<div class="aw ${kind}"></div>`);
    this.frame = document.createElement("iframe");
    this.frame.src = `frame.html?kind=${kind}`;
    this.frame.width = kind === "hud" ? "360" : "420";
    this.frame.height = "640";
    this.el.append(this.frame);
    stage.append(this.el);
    this.loaded = new Promise((r) => this.frame.addEventListener("load", r, { once: true }));
    this.key = null;
  }

  async ready() {
    await this.loaded;
    this.doc = this.frame.contentDocument;
    this.root = this.doc.getElementById(this.kind === "hud" ? "slip" : "panel");
    this.doc.getElementById(this.kind === "hud" ? "panel" : "slip").remove();
    this.root.hidden = false;
    await this.doc.fonts.load('600 16px "Ysabeau Office"', "Aa");
    await this.doc.fonts.load('600 16px "Ysabeau Office"', "Аа");
    await this.doc.fonts.ready;
  }

  /** Rebuilds the inside only when the state changes, like the app's render(). */
  render(key, html) {
    if (key === this.key) return false;
    this.key = key;
    this.root.innerHTML = html;
    return true;
  }

  $(sel) {
    return this.doc.querySelector(sel);
  }

  size() {
    const r = this.root.getBoundingClientRect();
    return { w: Math.ceil(r.width), h: Math.ceil(r.height) };
  }

  /** Shows the window at x, y (its top left) sized to its content. */
  show(x, y, opacity = 1) {
    const { w, h: height } = this.size();
    Object.assign(this.el.style, {
      visibility: opacity > 0 ? "visible" : "hidden",
      opacity: String(opacity),
      left: `${Math.round(x)}px`,
      top: `${Math.round(y)}px`,
      width: `${w}px`,
      height: `${height}px`,
    });
  }

  hide() {
    this.el.style.visibility = "hidden";
    this.key = null;
  }

  /** Where an element inside the window is, in stage coordinates. */
  at(sel) {
    const el = this.$(sel);
    if (!el) return null;
    const r = el.getBoundingClientRect();
    const b = box(this.el);
    return { x: b.x + r.left, y: b.y + r.top, w: r.width, h: r.height };
  }

  /** Sets :hover, :active, :focus-visible by hand: { selector: ["hover", "focus", ...] }. */
  states(map) {
    for (const el of this.doc.querySelectorAll("[data-hover],[data-press],[data-focus]")) {
      delete el.dataset.hover;
      delete el.dataset.press;
      delete el.dataset.focus;
    }
    for (const [sel, list] of Object.entries(map)) {
      const el = this.$(sel);
      if (el) for (const s of list) el.dataset[s] = "";
    }
  }
}

/** The keys being pressed, in the corner of the scene. A pressed key is marked. */
class Keys {
  constructor(labels, { right = 28, bottom = 26 } = {}) {
    this.el = h(`<div class="keys">${labels.map((l) => `<span class="key">${l}</span>`).join("")}</div>`);
    Object.assign(this.el.style, { right: `${right}px`, bottom: `${bottom}px` });
    stage.append(this.el);
    this.caps = [...this.el.children];
  }

  /** Keys go down one after another from `down`, all come up at `up`, the row leaves at `leave`. */
  draw(t, { down, up, leave }) {
    const enter = down - 0.14;
    const k = ease(seg(t, enter, down)) * (1 - ease(seg(t, leave - 0.3, leave)));
    this.el.style.visibility = t >= enter && t < leave ? "visible" : "hidden";
    this.el.style.opacity = String(k);
    this.el.style.transform = `translateY(${(1 - k) * 6}px)`;
    this.caps.forEach((cap, i) => cap.classList.toggle("down", t >= down + i * 0.1 && t < up));
  }
}

const ARROW = `<svg class="pointer" viewBox="0 0 22 28" width="22" height="28">
  <path d="M3 2.5v19l4.8-4.4 3.2 7.5 3.2-1.4-3.1-7.2h6.5z" fill="#111" stroke="#fff" stroke-width="1.6" stroke-linejoin="round"/>
</svg>`;
const BEAM = `<svg class="pointer" viewBox="0 0 22 28" width="22" height="28">
  <path d="M7.5 4.5h2c.9 0 1.5.5 1.5 1.2 0-.7.6-1.2 1.5-1.2h2M11 5.7v16.6M7.5 23.5h2c.9 0 1.5-.5 1.5-1.2 0 .7.6 1.2 1.5 1.2h2" fill="none" stroke="#fff" stroke-width="3.6" stroke-linecap="round"/>
  <path d="M7.5 4.5h2c.9 0 1.5.5 1.5 1.2 0-.7.6-1.2 1.5-1.2h2M11 5.7v16.6M7.5 23.5h2c.9 0 1.5-.5 1.5-1.2 0 .7.6 1.2 1.5 1.2h2" fill="none" stroke="#111" stroke-width="1.5" stroke-linecap="round"/>
</svg>`;

/** The mouse. Moves are resolved when they start, so they can aim at what is on screen then. */
class Pointer {
  constructor(start, moves = [], clicks = [], beam = []) {
    this.arrow = h(ARROW);
    this.beam = h(BEAM);
    stage.append(this.arrow, this.beam);
    this.start = start;
    this.moves = moves; // { at, dur, to: () => {x, y} }
    this.clicks = clicks;
    this.beamAt = beam; // [from, to] spans when the pointer is a text cursor
  }

  where(t) {
    let p = this.start;
    for (const m of this.moves) {
      if (t < m.at) break;
      m.target ??= m.to();
      p = { x: lerp(p.x, m.target.x, glide(seg(t, m.at, m.at + m.dur))), y: lerp(p.y, m.target.y, glide(seg(t, m.at, m.at + m.dur))) };
    }
    return p;
  }

  draw(t) {
    const p = this.where(t);
    const isBeam = this.beamAt.some(([a, b]) => t >= a && t < b);
    const press = this.clicks.some((c) => t >= c && t < c + 0.14);
    this.arrow.style.display = isBeam ? "none" : "block";
    this.beam.style.display = isBeam ? "block" : "none";
    const [el, ox, oy] = isBeam ? [this.beam, 11, 14] : [this.arrow, 3, 2.5];
    el.style.transform = `translate(${(p.x - ox).toFixed(1)}px, ${(p.y - oy).toFixed(1)}px) scale(${press ? 0.88 : 1})`;
  }
}

function field(el, text, caret, placeholder = "Сообщение") {
  const c = `<span class="caret${caret ? "" : " off"}"></span>`;
  el.innerHTML = text ? `${esc(text)}${c}` : `${c}<span class="ph">${placeholder}</span>`;
}

/** The HUD under the text, as src/hud.ts and hud.css draw it. */
function drawHud(hud, t, { anchor, work, done, hide, words }) {
  if (t < work || t >= hide) return hud.hide();
  const finished = t >= done;
  const text = finished ? "Готово" : words;
  if (hud.render(text, `<span class="slip-text" id="text" data-text="${text}">${text}</span>`)) {
    hud.root.dataset.state = finished ? "done" : "working";
  }
  const from = sweep(done - work);
  const c = finished ? settle(from, seg(t, done, done + 0.22)) : sweep(t - work);
  hud.$(".slip-text").style.setProperty("--clip", clip(c));
  const a = anchor();
  hud.show(a.x, a.y + a.h + 8, ease(seg(t, work, work + 0.22)) * (1 - seg(t, hide - 0.1, hide)));
}

// ---------- the scenes ----------

const scenes = {};

/** Translate: write in your language, press the keys, the field is in English. */
scenes.translate = (() => {
  const RU = "Да, собрал час назад. Скину ссылку, как только загрузится";
  const EN = "Yes, built it an hour ago. I'll send the link once it's uploaded.";
  const typing = typist(RU, 0.7, 23, 11);
  const DOWN = typing.end + 0.5;
  const UP = DOWN + 0.42;
  const WORK = UP + 0.06;
  const DONE = WORK + 1.62;
  const HIDE = DONE + 0.9;
  const SEND = HIDE + 0.55;
  const RESET = SEND + 1.7;
  let win, fieldEl, sent, sentH, hud, keys, enter;

  return {
    w: 800,
    h: 500,
    duration: RESET + 0.5,
    async build() {
      win = h(`
        <div class="win" style="left:70px;top:36px;width:660px;height:330px">
          <div class="head"><b>Tom Becker</b><span>в сети</span></div>
          <div class="chat-body">
            <div class="msg out">Morning! Fixing the last bug, will ping you soon.</div>
            <div class="msg">Hey! Is the new build ready? I'd like to try it before the call.</div>
            <div class="msg out" id="sent">${esc(EN)}</div>
          </div>
          <div class="chat-foot"><div class="field" id="field"></div></div>
        </div>`);
      stage.append(win);
      fieldEl = win.querySelector("#field");
      sent = win.querySelector("#sent");
      sentH = sent.offsetHeight;
      hud = new AirWindow("hud");
      keys = new Keys(["Ctrl", "Alt", "T"]);
      enter = new Keys(["Enter"]);
      await hud.ready();
    },
    seek(t) {
      let text = "";
      let since = 0;
      if (t < DONE) {
        text = typing.at(t);
        since = typing.last(t);
      } else if (t < SEND) {
        text = EN;
        since = DONE;
      } else {
        since = SEND;
      }
      field(fieldEl, text, caretOn(t, since));

      const k = ease(seg(t, SEND, SEND + 0.24)) * (1 - ease(seg(t, RESET, RESET + 0.4)));
      Object.assign(sent.style, {
        display: k > 0 ? "block" : "none",
        height: `${sentH * k}px`,
        marginTop: `${-8 * (1 - k)}px`,
        opacity: String(k),
        overflow: "hidden",
      });

      drawHud(hud, t, { anchor: () => box(fieldEl), work: WORK, done: DONE, hide: HIDE, words: "Перевожу…" });
      keys.draw(t, { down: DOWN, up: UP, leave: DONE + 0.5 });
      enter.draw(t, { down: SEND - 0.12, up: SEND + 0.06, leave: SEND + 0.8 });
    },
  };
})();

/** Fix: a letter with mistakes, one keypress, the squiggles are gone. */
scenes.fix = (() => {
  const DRAFT = [
    "Хочу взять отпуск с 15 по 26 декабря. Все задачи до этого ",
    ["закрою а"],
    " срочные вопросы передам Олегу. Подскажите ",
    ["пожалуйста"],
    " какое ",
    ["заявлние"],
    " нужно заполнить и до какого числа его ",
    ["отправить"],
  ];
  const FIXED =
    "Хочу взять отпуск с 15 по 26 декабря. Все задачи до этого закрою, а срочные вопросы передам Олегу. Подскажите, пожалуйста, какое заявление нужно заполнить и до какого числа его отправить?";
  const DOWN = 1.0;
  const UP = DOWN + 0.42;
  const WORK = UP + 0.06;
  const DONE = WORK + 1.62;
  const HIDE = DONE + 0.9;
  const RESET = HIDE + 2.0;
  let win, body, hud, keys;

  const draft = DRAFT.map((p) => (Array.isArray(p) ? `<span class="typo">${esc(p[0])}</span>` : esc(p))).join("");

  return {
    w: 800,
    h: 440,
    duration: RESET + 0.5,
    async build() {
      win = h(`
        <div class="win" style="left:70px;top:34px;width:660px">
          <div class="head"><b>Новое письмо</b></div>
          <div class="mail-row"><span>Кому</span><span>Анна Петрова</span></div>
          <div class="mail-row"><span>Тема</span><span>Отпуск в декабре</span></div>
          <div class="mail-body" id="body"></div>
        </div>`);
      stage.append(win);
      body = win.querySelector("#body");
      hud = new AirWindow("hud");
      keys = new Keys(["Ctrl", "Alt", "R"]);
      await hud.ready();
    },
    seek(t) {
      const fixed = t >= DONE && t < RESET + 0.25;
      const caret = `<span class="caret${caretOn(t, fixed ? DONE : 0) ? "" : " off"}"></span>`;
      body.innerHTML = `Анна, добрый день!\n\n${fixed ? esc(FIXED) : draft}${caret}`;
      body.style.opacity = String(1 - Math.min(seg(t, RESET, RESET + 0.25), 1 - seg(t, RESET + 0.25, RESET + 0.5)));

      drawHud(hud, t, { anchor: () => box(body), work: WORK, done: DONE, hide: HIDE, words: "Исправляю…" });
      keys.draw(t, { down: DOWN, up: UP, leave: DONE + 0.5 });
    },
  };
})();

/** Improve: a rushed message comes out clear and calm. */
scenes.improve = (() => {
  const TYPED = "короче глянул, в целом норм, но кнопка на втором экране какая то потерянная и текст мелкий, ";
  const REST = "надо бы поправить до пятницы наверно";
  const BETTER = "Посмотрел, в целом хорошо. До пятницы стоит поправить две вещи: кнопка на втором экране теряется, а текст там мелковат.";
  const typing = typist(REST, 0.5, 24, 5);
  const DOWN = typing.end + 0.5;
  const UP = DOWN + 0.42;
  const WORK = UP + 0.06;
  const DONE = WORK + 1.62;
  const HIDE = DONE + 0.9;
  const SEND = HIDE + 0.55;
  const RESET = SEND + 1.8;
  let win, fieldEl, sent, sentH, hud, keys, enter;

  return {
    w: 800,
    h: 500,
    duration: RESET + 0.5,
    async build() {
      win = h(`
        <div class="win" style="left:70px;top:30px;width:660px;height:360px">
          <div class="head"><b>Лена Соколова</b><span>в сети</span></div>
          <div class="chat-body">
            <div class="msg">Привет!</div>
            <div class="msg">Глянь, пожалуйста, макет онбординга. Хочу в пятницу отдать его в разработку</div>
            <div class="msg out" id="sent">${esc(BETTER)}</div>
          </div>
          <div class="chat-foot"><div class="field" id="field"></div></div>
        </div>`);
      stage.append(win);
      fieldEl = win.querySelector("#field");
      sent = win.querySelector("#sent");
      sentH = sent.offsetHeight;
      hud = new AirWindow("hud");
      keys = new Keys(["Ctrl", "Alt", "E"]);
      enter = new Keys(["Enter"]);
      await hud.ready();
    },
    seek(t) {
      let text = "";
      let since = 0;
      if (t < DONE) {
        text = TYPED + typing.at(t);
        since = typing.last(t);
      } else if (t < SEND) {
        text = BETTER;
        since = DONE;
      } else if (t >= RESET + 0.25) {
        text = TYPED;
        since = RESET + 0.25;
      } else {
        since = SEND;
      }
      field(fieldEl, text, caretOn(t, since));
      fieldEl.style.opacity = t >= RESET ? String(Math.max(1 - seg(t, RESET, RESET + 0.25), seg(t, RESET + 0.25, RESET + 0.5))) : "1";

      const k = ease(seg(t, SEND, SEND + 0.24)) * (1 - ease(seg(t, RESET, RESET + 0.4)));
      Object.assign(sent.style, {
        display: k > 0 ? "block" : "none",
        height: `${sentH * k}px`,
        marginTop: `${-8 * (1 - k)}px`,
        opacity: String(k),
        overflow: "hidden",
      });

      drawHud(hud, t, { anchor: () => box(fieldEl), work: WORK, done: DONE, hide: HIDE, words: "Улучшаю…" });
      keys.draw(t, { down: DOWN, up: UP, leave: DONE + 0.5 });
      enter.draw(t, { down: SEND - 0.12, up: SEND + 0.06, leave: SEND + 0.8 });
    },
  };
})();

/** Write for me: an empty field, a request in your own words, the reply goes in. */
scenes.compose = (() => {
  const REQUEST = "откажись от пятницы, предложи понедельник после двух";
  const ANSWER = "Привет! В пятницу не получится, весь день расписан. Давай перенесём на понедельник? После двух мне удобно в любое время.";
  const TONES = ["обычно", "вежливо", "дружелюбно", "по-деловому", "коротко", "с юмором"];
  const DOWN = 0.7;
  const UP = DOWN + 0.42;
  const OPEN = UP + 0.08;
  const typing = typist(REQUEST, OPEN + 0.45, 28, 3);
  const TONE = typing.end + 0.8; // click on "обычно"
  const PICK = TONE + 0.85; // click on "дружелюбно"
  const WRITE = PICK + 0.85; // click on "Написать"
  const RESULT = WRITE + 1.62;
  const PASTE = RESULT + 1.4; // click on "Вставить"
  const RESET = PASTE + 1.9;
  let win, fieldEl, panel, keys, pointer;

  const ask = (text, tone, picking, caret) => `
    <div class="input request" id="request" data-focus>${text ? esc(text) : ""}<span class="caret${caret ? "" : " off"}"></span>${
      text ? "" : '<span class="hint">Что написать? Например: вежливо откажись от встречи в пятницу</span>'
    }</div>
    <div class="bar">
      <p class="sentence"><span class="plain">На </span><button class="word" id="lang">языке запроса</button><span class="plain comma">,</span><button class="word" id="tone" aria-expanded="${picking}">${tone}</button></p>
      <button class="btn-mark" id="go">Написать</button>
    </div>
    ${
      picking
        ? `<div class="choices">${TONES.map((x) => `<button class="choice" id="t-${x}" aria-selected="${x === tone}">${x}</button>`).join("")}</div>`
        : ""
    }`;

  return {
    w: 800,
    h: 500,
    duration: RESET + 0.5,
    async build() {
      win = h(`
        <div class="win" style="left:70px;top:20px;width:660px;height:250px">
          <div class="head"><b>Маша</b><span>в сети</span></div>
          <div class="chat-body">
            <div class="msg">Слушай, а в пятницу в 11 встречаемся, как договаривались? Хочу показать новый макет</div>
          </div>
          <div class="chat-foot"><div class="field" id="field"></div></div>
        </div>`);
      stage.append(win);
      fieldEl = win.querySelector("#field");
      panel = new AirWindow("panel");
      keys = new Keys(["Ctrl", "Alt", "Space"]);
      pointer = new Pointer(
        { x: 760, y: 230 },
        [
          { at: TONE - 0.62, dur: 0.55, to: () => center(panel.at("#tone")) },
          { at: PICK - 0.62, dur: 0.55, to: () => center(panel.at("#t-дружелюбно")) },
          { at: WRITE - 0.62, dur: 0.55, to: () => center(panel.at("#go")) },
          { at: PASTE - 0.7, dur: 0.6, to: () => center(panel.at("#put")) },
          { at: PASTE + 0.5, dur: 0.9, to: () => ({ x: 760, y: 230 }) },
        ],
        [TONE, PICK, WRITE, PASTE],
      );
      await panel.ready();
    },
    seek(t) {
      // The field: empty until the answer is put in.
      const pasted = t >= PASTE + 0.08 && t < RESET + 0.25;
      // While the panel is open it has the focus, so the field shows no caret.
      const away = t >= OPEN && t < PASTE + 0.08;
      field(fieldEl, pasted ? ANSWER : "", !away && caretOn(t, pasted ? PASTE : 0));
      fieldEl.style.opacity = t >= RESET ? String(Math.max(1 - seg(t, RESET, RESET + 0.25), seg(t, RESET + 0.25, RESET + 0.5))) : "1";

      keys.draw(t, { down: DOWN, up: UP, leave: OPEN + 0.9 });

      if (t >= OPEN && t < PASTE + 0.06) {
        let since = OPEN;
        if (t < WRITE) {
          const text = typing.at(t);
          const tone = t >= PICK ? "дружелюбно" : "обычно";
          const picking = t >= TONE && t < PICK;
          const caret = caretOn(t, typing.last(t));
          panel.render(`ask ${text} ${tone} ${picking} ${caret}`, ask(text, tone, picking, caret));
          panel.states({
            "#request": picking ? [] : ["focus"],
            "#tone": t >= TONE - 0.07 && t < TONE + 0.1 ? ["hover"] : [],
            "#t-дружелюбно": t >= PICK - 0.07 && t < PICK ? ["hover"] : [],
            [picking ? '.choice[aria-selected="true"]' : "#none"]: t < PICK - 0.07 ? ["focus"] : [],
            "#go": t >= WRITE - 0.07 ? ["hover"].concat(t >= WRITE ? ["press"] : []) : [],
          });
        } else if (t < RESULT) {
          since = WRITE;
          panel.render("working", `<p class="asked">${esc(REQUEST)}</p><p class="working"><span class="working-text" data-text="Пишу…">Пишу…</span></p>`);
          panel.$(".working-text").style.setProperty("--clip", clip(sweep(t - WRITE)));
        } else {
          since = RESULT;
          panel.render(
            "result",
            `<div class="answer">${esc(ANSWER)}</div>
             <div class="bar"><button class="btn-mark" id="put">Вставить</button>
               <div class="tools"><button class="link">Скопировать</button><button class="link">Ещё раз</button><button class="link">Изменить</button></div></div>`,
          );
          panel.states({ "#put": t >= PASTE - 0.08 ? ["hover"].concat(t >= PASTE ? ["press"] : []) : [] });
        }
        // Every new state fades in, as `.panel > *` does in panel.css.
        panel.root.style.opacity = since === OPEN ? "1" : String(ease(seg(t, since, since + 0.15)));
        const a = box(fieldEl);
        panel.show(a.x, a.y + a.h + 8, ease(seg(t, OPEN, OPEN + 0.15)));
      } else {
        panel.hide();
      }
      pointer.draw(t);
    },
  };
})();

/** Read: select text on a page you can't edit, get it in your language in the panel. */
scenes.read = (() => {
  const PARA =
    "From 1 November the museum closes at 6 pm on weekdays. Last entry is an hour before closing, and the café on the ground floor stays open until 7 pm.";
  const RU = "С 1 ноября по будням музей закрывается в 18:00. Последний вход за час до закрытия, а кафе на первом этаже работает до 19:00.";
  const words = PARA.split(" ");
  const SELECT = 0.7;
  const SELECTED = SELECT + 1.1;
  const DOWN = SELECTED + 0.45;
  const UP = DOWN + 0.42;
  const OPEN = UP + 0.08;
  const RUN = OPEN + 1.25; // click on "Перевести"
  const RESULT = RUN + 1.62;
  const COPY = RESULT + 1.35; // click on "Скопировать"
  const CLOSE = COPY + 0.45;
  const CLEAR = CLOSE + 1.3;
  const END = CLEAR + 0.9;
  let win, spans, panel, keys, pointer;

  const verbs = `
    <p class="quote" style="--lines: 2"><mark>${esc(PARA)}</mark></p>
    <div class="verbs" role="toolbar">
      <span class="verb-group"><button class="verb" id="translate">Перевести</button><span class="plain"> на </span><button class="word" aria-expanded="false">Русский</button><span class="plain comma">,</span></span>
      <span class="verb-group"><button class="verb">исправить</button><span class="plain comma">,</span></span>
      <span class="verb-group"><button class="verb">улучшить</button></span>
      <span class="verb-group"><span class="plain">или </span><button class="verb">вежливее</button></span>
    </div>
    <input class="input" placeholder="Или скажи, что сделать с текстом" />`;

  const quote1 = `<p class="quote" style="--lines: 1"><mark>${esc(PARA)}</mark></p>`;

  return {
    w: 800,
    h: 500,
    duration: END,
    async build() {
      win = h(`
        <div class="win" style="left:50px;top:20px;width:700px;height:560px">
          <div class="url"><span>museum.example.org/visit</span></div>
          <article>
            <h1>Plan your visit</h1>
            <p id="para">${words.map((w) => `<span class="w">${esc(w)}</span>`).join('<span class="w"> </span>')}</p>
          </article>
        </div>`);
      stage.append(win);
      spans = [...win.querySelectorAll("#para .w")];
      panel = new AirWindow("panel");
      keys = new Keys(["Ctrl", "Alt", "Space"]);
      const first = box(spans[0]);
      const rest = { x: 640, y: 110 };
      pointer = new Pointer(
        rest,
        [
          { at: 0.05, dur: 0.6, to: () => ({ x: first.x + 1, y: first.y + first.h / 2 }) },
          ...spans.map((s, i) => ({
            at: SELECT + (i / spans.length) * (SELECTED - SELECT),
            dur: (SELECTED - SELECT) / spans.length,
            to: () => {
              const r = box(s);
              return { x: r.x + r.w, y: r.y + r.h / 2 };
            },
          })),
          { at: RUN - 0.7, dur: 0.6, to: () => center(panel.at("#translate")) },
          { at: COPY - 0.75, dur: 0.6, to: () => center(panel.at("#copy")) },
          { at: CLOSE + 0.3, dur: 0.7, to: () => ({ x: 640, y: 470 }) },
          { at: CLEAR - 0.05, dur: 0.85, to: () => rest },
        ],
        [SELECT, RUN, COPY, CLEAR - 0.1],
        [
          [0, RUN - 0.6],
          [CLEAR + 0.4, END + 1],
        ],
      );
      await panel.ready();
    },
    seek(t) {
      const n = t >= CLEAR - 0.1 ? 0 : Math.round(seg(t, SELECT, SELECTED) * spans.length);
      spans.forEach((s, i) => s.classList.toggle("sel", i < n));

      keys.draw(t, { down: DOWN, up: UP, leave: OPEN + 0.9 });

      if (t >= OPEN && t < CLOSE) {
        let since = OPEN;
        if (t < RUN) {
          panel.render("ask", verbs);
          panel.states({ "#translate": ["focus"].concat(t >= RUN - 0.1 ? ["hover"] : []) });
        } else if (t < RESULT) {
          since = RUN;
          panel.render("working", `${quote1}<p class="working"><span class="working-text" data-text="Перевожу…">Перевожу…</span></p>`);
          panel.$(".working-text").style.setProperty("--clip", clip(sweep(t - RUN)));
        } else {
          since = RESULT;
          const copied = t >= COPY;
          panel.render(
            `result ${copied}`,
            `${quote1}<div class="answer">${esc(RU)}</div>
             <div class="bar"><button class="btn-mark" id="copy">${copied ? "Скопировано" : "Скопировать"}</button>
               <div class="tools"><button class="link">Ещё раз</button><button class="link">Назад</button></div></div>`,
          );
          panel.states({ "#copy": t >= COPY - 0.1 ? ["hover"].concat(t >= COPY && t < COPY + 0.14 ? ["press"] : []) : [] });
          if (copied) since = RESULT - 1; // the label changes in place, nothing fades
        }
        panel.root.style.opacity = since === OPEN ? "1" : String(ease(seg(t, since, since + 0.15)));
        const a = box(spans[0]);
        const b = box(spans[spans.length - 1]);
        panel.show(a.x, b.y + b.h + 8, ease(seg(t, OPEN, OPEN + 0.15)));
      } else {
        panel.hide();
      }
      pointer.draw(t);
    },
  };
})();

/** The settings window, twice: your own actions, and the model on your computer. */
scenes.settings = (() => {
  let frames;
  return {
    w: 920,
    h: 712,
    duration: 0,
    async build() {
      frames = ["actions", "model"].map((view, i) => {
        const el = h(`<div class="aw settings" style="left:${26 + i * 448}px;top:36px;width:420px;height:640px"></div>`);
        const frame = document.createElement("iframe");
        // The app built by render.cjs, with tauri-mock.js standing in for the Rust side.
        frame.src = "/app/index.html";
        // The narrowest the window goes, so the text stays readable in the README.
        frame.width = "420";
        frame.height = "640";
        el.append(frame);
        stage.append(el);
        el.style.visibility = "visible";
        return { view, frame, loaded: new Promise((r) => frame.addEventListener("load", r, { once: true })) };
      });
      for (const { view, frame, loaded } of frames) {
        await loaded;
        const doc = frame.contentDocument;
        await doc.fonts.ready;
        // Let the page finish its start-up, then open the tab.
        await new Promise((r) => setTimeout(r, 300));
        doc.getElementById(`tab-${view}`).click();
        doc.activeElement?.blur();
        await doc.fonts.ready;
      }
      await new Promise((r) => setTimeout(r, 400));
    },
    seek() {},
  };
})();

// ---------- the page ----------

const name = new URLSearchParams(location.search).get("scene") ?? "translate";
const scene = scenes[name];
stage.style.width = `${scene.w}px`;
stage.style.height = `${scene.h}px`;

window.scene = { name, w: scene.w, h: scene.h, duration: scene.duration, all: Object.keys(scenes) };
window.ready = (async () => {
  await document.fonts.ready;
  await scene.build();
  await document.fonts.ready;
  scene.seek(0);
})();
window.seek = (t) => scene.seek(t);
