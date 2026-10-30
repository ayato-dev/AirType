// Renders the README animations into docs/media as animated WebP, light and dark.
//   node docs/scenes/render.cjs                 every scene
//   node docs/scenes/render.cjs translate fix   only these
//   node docs/scenes/render.cjs --still read 4.2   one PNG frame, to check a moment
//   SCHEMES=light node docs/scenes/render.cjs fix  one theme only
// Needs Playwright with Chromium, ffmpeg built with libwebp, and `npm install` done:
// the settings picture is the real settings page, built with Vite.
const { chromium } = require("playwright");
const http = require("http");
const fs = require("fs");
const os = require("os");
const path = require("path");
const { execFileSync } = require("child_process");

const ROOT = path.resolve(__dirname, "../..");
const OUT = path.join(ROOT, "docs/media");
const FPS = 25;
const SCALE = 2;

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css",
  ".js": "text/javascript",
  ".woff2": "font/woff2",
};

/** The app's pages built with relative paths, served under /app/. */
function buildApp() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "airtype-app-"));
  execFileSync("npx", ["vite", "build", "--base", "./", "--outDir", dir, "--emptyOutDir", "--logLevel", "error"], {
    cwd: ROOT,
    stdio: "inherit",
  });
  return dir;
}

function serve(app) {
  const server = http.createServer((req, res) => {
    const url = decodeURIComponent(new URL(req.url, "http://x").pathname);
    const [base, rel] = url.startsWith("/app/") ? [app, url.slice(5)] : [ROOT, url];
    const file = path.join(base, rel);
    if (!base || !file.startsWith(base)) {
      res.writeHead(403).end();
      return;
    }
    fs.readFile(file, (err, body) => {
      if (err) res.writeHead(404).end();
      else res.writeHead(200, { "content-type": TYPES[path.extname(file)] ?? "application/octet-stream" }).end(body);
    });
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

async function open(browser, base, name, scheme) {
  const page = await browser.newPage({ viewport: { width: 1000, height: 600 }, deviceScaleFactor: SCALE, colorScheme: scheme });
  await page.addInitScript({ path: path.join(__dirname, "tauri-mock.js") });
  await page.goto(`${base}/docs/scenes/stage.html?scene=${name}`);
  await page.evaluate(() => window.ready);
  const scene = await page.evaluate(() => window.scene);
  await page.setViewportSize({ width: scene.w, height: scene.h });
  return { page, scene };
}

async function main() {
  const args = process.argv.slice(2);
  const app = args.length === 0 || args.includes("settings") ? buildApp() : null;
  const server = await serve(app);
  const base = `http://127.0.0.1:${server.address().port}`;
  const browser = await chromium.launch();
  fs.mkdirSync(OUT, { recursive: true });

  if (args[0] === "--still") {
    const [, name, at, scheme = "light"] = args;
    const { page } = await open(browser, base, name, scheme);
    for (let t = 0; t <= Number(at); t += 1 / FPS) await page.evaluate((x) => window.seek(x), t);
    const file = path.join(os.tmpdir(), `${name}-${at}-${scheme}.png`);
    await page.screenshot({ path: file, omitBackground: true });
    console.log(file);
  } else {
    const { page: probe, scene: first } = await open(browser, base, "translate", "light");
    const names = args.length ? args : first.all;
    await probe.close();
    for (const name of names) {
      for (const scheme of (process.env.SCHEMES ?? "light,dark").split(",")) {
        const { page, scene } = await open(browser, base, name, scheme);
        if (scene.duration === 0) {
          // A still picture.
          const png = path.join(os.tmpdir(), `airtype-${name}-${scheme}.png`);
          await page.screenshot({ path: png, omitBackground: true });
          await page.close();
          const file = path.join(OUT, `${name}-${scheme}.webp`);
          execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-i", png, "-c:v", "libwebp", "-lossless", "1", "-compression_level", "6", file]);
          fs.rmSync(png);
          console.log(`${path.relative(ROOT, file)}  ${(fs.statSync(file).size / 1024).toFixed(0)} KB`);
          continue;
        }
        const dir = fs.mkdtempSync(path.join(os.tmpdir(), `airtype-${name}-`));
        const frames = Math.round(scene.duration * FPS);
        // In order: the pointer aims at what is on screen when each move starts.
        for (let i = 0; i < frames; i++) {
          await page.evaluate((t) => window.seek(t), i / FPS);
          await page.screenshot({ path: path.join(dir, `${String(i).padStart(4, "0")}.png`), omitBackground: true });
        }
        await page.close();
        const file = path.join(OUT, `${name}-${scheme}.webp`);
        execFileSync("ffmpeg", [
          "-y", "-loglevel", "error",
          "-framerate", String(FPS),
          "-i", path.join(dir, "%04d.png"),
          // Lossless: flat UI compresses well this way, and lossy blurs the marker's edges.
          "-c:v", "libwebp_anim",
          "-lossless", "1",
          "-compression_level", "4",
          "-loop", "0",
          file,
        ]);
        fs.rmSync(dir, { recursive: true, force: true });
        console.log(`${path.relative(ROOT, file)}  ${frames} frames  ${(fs.statSync(file).size / 1024).toFixed(0)} KB`);
      }
    }
  }
  await browser.close();
  server.close();
  if (app) fs.rmSync(app, { recursive: true, force: true });
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
