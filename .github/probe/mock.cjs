// Mock OpenAI-compatible server for the PR #8 probe. No keys, no model.
const http = require('http');
const fs = require('fs');
const path = require('path');

const OUT = process.argv[2] || '.';
const LOG = path.join(OUT, 'mock.log');
const log = (o) => fs.appendFileSync(LOG, JSON.stringify({ t: new Date().toISOString(), ...o }) + '\n');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
// ASCII on the wire, so the drivers don't have to agree on an encoding.
const ascii = (o) =>
  JSON.stringify(o).replace(/[\u0080-￿]/g, (c) => '\\u' + c.charCodeAt(0).toString(16).padStart(4, '0'));

const SCENARIOS = {
  stream_line: { kind: 'stream', deltas: ['ПОТОК: Прив', 'ет, див', 'ный мир'] },
  stream_lines: { kind: 'stream', deltas: ['ПОТОК строка ', 'один\nПОТОК ', 'строка два'] },
  json_line: { kind: 'json', text: 'ЦЕЛИКОМ: ответ одним куском' },
  json_lines: { kind: 'json', text: 'ЦЕЛИКОМ строка один\nЦЕЛИКОМ строка два' },
  cut_end: { kind: 'cut', how: 'end', deltas: ['ОБРЫВ на пол', 'услове'] },
  cut_reset: { kind: 'cut', how: 'reset', deltas: ['ОБРЫВ на пол', 'услове'] },
  refuse_stream: { kind: 'refuse', text: 'БЕЗ ПОТОКА строка один\nБЕЗ ПОТОКА строка два' },
};
const expected = (s) => s.text ?? s.deltas.join('');
const wantFlags = (s) => (s.kind === 'refuse' ? 'true,false' : 'true');

process.on('uncaughtException', (e) => log({ event: 'uncaught', message: String(e) }));

let mode = 'stream_line';
let started = 0;
let completed = 0;
let page = null;
let pending = null;
let flags = [];
let selected = null;
let pageEvents = [];
let delay = 300;

const event = (data) => `data: ${typeof data === 'string' ? data : JSON.stringify(data)}\n\n`;
const chunk = (delta, finish) => ({
  id: 'probe',
  object: 'chat.completion.chunk',
  model: 'mock',
  choices: [{ index: 0, delta, finish_reason: finish ?? null }],
});

// One event in two TCP writes, cut inside the first multi-byte character.
async function writeSplit(res, text, splits) {
  const bytes = Buffer.from(text);
  const lead = bytes.findIndex((b) => b >= 0xc0);
  if (lead < 0) {
    res.write(bytes);
    return;
  }
  res.write(bytes.subarray(0, lead + 1));
  splits.push(`${bytes[lead].toString(16)}|${bytes[lead + 1].toString(16)}`);
  await sleep(70);
  res.write(bytes.subarray(lead + 1));
}

async function chat(req, res, raw) {
  started += 1;
  const scenario = SCENARIOS[mode];
  let body = {};
  try {
    body = JSON.parse(raw);
  } catch {}
  const user = body.messages?.find((m) => m.role === 'user')?.content ?? '';
  log({
    event: 'request',
    n: started,
    mode,
    path: req.url,
    stream: body.stream,
    model: body.model,
    selected: user.match(/<text>\n([\s\S]*?)\n<\/text>/)?.[1] ?? null,
    authorization: req.headers.authorization ? 'present' : 'absent',
  });
  flags.push(String(body.stream));
  selected = user.match(/<text>\n([\s\S]*?)\n<\/text>/)?.[1] ?? null;
  req.socket.setNoDelay(true);
  if (scenario.kind === 'refuse' && body.stream === true) {
    res.writeHead(400, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ error: { message: 'Your organization must be verified to stream this model.' } }));
    completed += 1;
    log({ event: 'done', n: started, mode, kind: 'refuse', how: '400 to stream:true' });
    return;
  }
  const splits = [];
  if (scenario.kind === 'json' || scenario.kind === 'refuse') {
    await sleep(delay);
    const whole = {
      id: 'probe',
      object: 'chat.completion',
      model: 'mock',
      choices: [{ index: 0, message: { role: 'assistant', content: scenario.text }, finish_reason: 'stop' }],
    };
    res.writeHead(200, { 'content-type': 'application/json; charset=utf-8' });
    res.end(JSON.stringify(whole));
  } else {
    res.writeHead(200, { 'content-type': 'text/event-stream; charset=utf-8', 'cache-control': 'no-cache' });
    res.flushHeaders();
    res.write(': keep-alive\n\n');
    res.write(event(chunk({ role: 'assistant', content: '' })));
    for (const content of scenario.deltas) {
      await sleep(150);
      await writeSplit(res, event(chunk({ content })), splits);
    }
    await sleep(150);
    if (scenario.how === 'end') {
      res.end();
    } else if (scenario.how === 'reset') {
      req.socket.destroy();
    } else {
      res.write(event(chunk({}, 'stop')));
      await sleep(50);
      res.write(event('[DONE]'));
      res.end();
    }
  }
  completed += 1;
  log({ event: 'done', n: started, mode, kind: scenario.kind, how: scenario.how ?? 'finish+DONE', utf8_split_lead_cont: splits });
}

const PAGE = `<!doctype html><meta charset="utf-8"><title>probe page</title>
<body style="margin:12px;font:18px sans-serif">
<textarea id="t" style="width:90vw;height:60vh;font-size:18px"></textarea>
<script>
const t = document.getElementById('t');
let last = '';
setInterval(async () => {
  try {
    const c = await (await fetch('/_cmd')).json();
    if (c && c.value !== undefined) { t.value = c.value; t.focus(); t.setSelectionRange(c.a, c.b); }
  } catch (e) {}
  const now = JSON.stringify({ value: t.value, a: t.selectionStart, b: t.selectionEnd, focus: document.hasFocus() });
  if (now !== last) { last = now; fetch('/_report', { method: 'POST', body: now }).catch(() => {}); }
}, 100);
t.focus();
const note = (o) => fetch('/_pageevent', { method: 'POST', body: JSON.stringify(o) }).catch(() => {});
addEventListener('keydown', (e) => { if (e.metaKey || e.ctrlKey) note({ ev: 'keydown', code: e.code }); }, true);
addEventListener('copy', () => note({ ev: 'copy' }), true);
addEventListener('beforeinput', (e) => note({ ev: 'beforeinput', code: e.inputType }), true);
addEventListener('input', (e) => note({ ev: 'input', code: e.inputType + '/' + t.value.length }), true);
addEventListener('blur', () => note({ ev: 'windowblur' }));
t.addEventListener('blur', () => note({ ev: 'fieldblur' }));
addEventListener('paste', (e) => note({ ev: 'paste', text: e.clipboardData.getData('text/plain').slice(0, 30) }), true);
</script>`;

http
  .createServer((req, res) => {
    const url = new URL(req.url, 'http://localhost');
    let raw = '';
    req.setEncoding('utf8');
    req.on('data', (d) => (raw += d));
    req.on('end', () => {
      const send = (o) => {
        res.writeHead(200, { 'content-type': 'application/json; charset=utf-8' });
        res.end(ascii(o));
      };
      if (url.pathname === '/v1/chat/completions') {
        chat(req, res, raw).catch((e) => log({ event: 'error', message: String(e) }));
      } else if (url.pathname === '/_mode') {
        mode = url.searchParams.get('m');
        delay = Number(url.searchParams.get('delay') ?? 300);
        flags = [];
        selected = null;
        pageEvents = [];
        log({ event: 'mode', mode });
        send({ mode, expected: expected(SCENARIOS[mode]), wantFlags: wantFlags(SCENARIOS[mode]), started, completed });
      } else if (url.pathname === '/_state') {
        send({ mode, started, completed, page, flags: flags.join(','), selected, pageEvents: pageEvents.join(' ') });
      } else if (url.pathname === '/_page') {
        res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
        res.end(PAGE);
      } else if (url.pathname === '/_report') {
        page = JSON.parse(raw);
        send({});
      } else if (url.pathname === '/_pageevent') {
        const e = JSON.parse(raw);
        log({ event: 'page', ...e });
        pageEvents.push(e.ev + (e.code ? ':' + e.code : '') + (e.text ? '=' + JSON.stringify(e.text) : ''));
        send({});
      } else if (url.pathname === '/_set') {
        pending = {
          value: url.searchParams.get('value'),
          a: Number(url.searchParams.get('a')),
          b: Number(url.searchParams.get('b')),
        };
        send({});
      } else if (url.pathname === '/_cmd') {
        const c = pending;
        pending = null;
        send(c);
      } else {
        res.writeHead(404);
        res.end();
      }
    });
  })
  .listen(8799);
log({ event: 'listening', port: 8799 });
