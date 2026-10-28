const { chromium } = require('playwright');
const fs = require('fs');
(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage({ deviceScaleFactor: 1 });
  for (const [svg, out, size] of process.argv.slice(2).map(a => a.split(':'))) {
    const n = Number(size);
    await page.setViewportSize({ width: n, height: n });
    await page.setContent(`<html><body style="margin:0;background:transparent"><img src="data:image/svg+xml;base64,${fs.readFileSync(svg).toString('base64')}" width="${n}" height="${n}"></body></html>`);
    await page.screenshot({ path: out, omitBackground: true });
  }
  await browser.close();
})();
