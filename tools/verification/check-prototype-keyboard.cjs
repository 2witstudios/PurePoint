// Isolated regression for revised prototypes; approved v3 evidence stays immutable.
const fs = require('fs');
const path = require('path');
const { chromium } = require(process.argv[2] || 'playwright');
(async () => {
  const browser = await chromium.launch({headless: true, ...(process.argv[3] ? {executablePath: process.argv[3]} : {})});
  try {
    const page = await browser.newPage({viewport: {width: 1280, height: 800}});
    for (const screen of ['channel', 'project', 'worktree']) {
      await page.setContent(fs.readFileSync(path.join(__dirname, '../../designs/agent-watch/v4-keyboard', screen + '.html'), 'utf8'));
      if (screen === 'worktree') await page.locator('#togglechannel').click();
      const more = page.locator('[data-more]').first();
      const id = await more.getAttribute('data-more');
      await more.click({force: true});
      if (!await page.locator(`[data-mark-unread="${id}"]`).evaluate(el => el === document.activeElement)) throw Error(screen + ': menu focus lost');
      await page.keyboard.press('Escape');
      const composer = page.locator('[data-compose="main"] textarea');
      await composer.fill('@I');
      if (!await page.locator('.mentionpicker').isVisible()) throw Error(screen + ': missing mention picker');
      await composer.evaluate(el => el.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', isComposing: true, bubbles: true, cancelable: true})));
      if (await composer.inputValue() !== '@I') throw Error(screen + ': IME selected mention');
      console.log('PASS ' + screen + ' menu keyboard focus and IME mention guard');
    }
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
