
import puppeteer from 'puppeteer-core';

const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const URL = 'http://127.0.0.1:8931/lab/tree/_viztest3.ipynb?token=viztest';

const browser = await puppeteer.launch({
  executablePath: CHROME, headless: true,
  args: ['--no-sandbox', '--disable-gpu', '--window-size=1400,1200'],
});
const page = await browser.newPage();
await page.setViewport({ width: 1400, height: 1200 });
page.on('console', (m) => { const t = m.text(); if (/error|Error|fail/i.test(t)) console.log('[console]', t.slice(0, 160)); });

await page.goto(URL, { waitUntil: 'networkidle2', timeout: 90000 });
await page.waitForSelector('.jp-Notebook', { timeout: 90000 });
console.log('notebook open');

// Dismiss the Jupyter news notification if it overlaps
await page.evaluate(() => {
  for (const b of document.querySelectorAll('button')) {
    if (b.textContent.trim() === 'No') { b.click(); }
  }
});
await new Promise((r) => setTimeout(r, 500));

// Command palette -> "Run All Cells"
await page.click('.jp-Notebook');
await page.keyboard.down('Control');
await page.keyboard.down('Shift');
await page.keyboard.press('KeyP');
await page.keyboard.up('Shift');
await page.keyboard.up('Control');
await page.waitForSelector('.lm-CommandPalette-search, [class*="CommandPalette"] input', { timeout: 15000 });
await page.keyboard.type('Run All Cells');
await new Promise((r) => setTimeout(r, 800));
await page.keyboard.press('Enter');
console.log('run-all issued via palette');

await page.waitForFunction(() => {
  const prompts = [...document.querySelectorAll('.jp-CodeCell .jp-InputPrompt')];
  if (!prompts.length) return false;
  const busy = prompts.some((p) => (p.textContent || '').includes('*'));
  return !busy && prompts.some((p) => /\[\d+\]/.test(p.textContent || ''));
}, { timeout: 300000, polling: 2000 });
await new Promise((r) => setTimeout(r, 8000));
console.log('execution finished');

const measure = () => page.evaluate(() => {
  const cells = [...document.querySelectorAll('.jp-CodeCell')];
  return cells.map((c, i) => {
    const nvl = c.querySelector('[id^="neo4j-viz-"]');
    const widget = c.querySelector('.jupyter-widget');
    const el = nvl || widget;
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return {
      cell: i,
      kind: nvl ? 'html' : 'widget',
      height: Math.round(r.height),
      canvases: el.querySelectorAll('canvas').length,
      visible: r.height > 50 && el.querySelectorAll('canvas').length > 0,
    };
  }).filter(Boolean);
});

const first = await measure();
await new Promise((r) => setTimeout(r, 8000));
const second = await measure();
for (let i = 0; i < first.length; i++) {
  const a = first[i]; const b = second[i];
  console.log(`viz cell ${a.cell} (${a.kind}): height ${a.height} -> ${b.height} (Δ${b.height - a.height}), canvases ${a.canvases}, painted=${a.visible}`);
}
if (!first.length) console.log('no viz elements found');
await page.screenshot({ path: '/tmp/jlab_live.png' });
await browser.close();
