import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { browser, $ } from '@wdio/globals';

describe('Skill Deck desktop', () => {
  it('opens the real main window and reads configuration through IPC', async () => {
    await $('header nav a[href="/settings"]').waitForDisplayed();
    const config = await browser.executeAsync((done) => {
      window.__TAURI_INTERNALS__.invoke('get_config').then(done, (error) => done({ error }));
    });
    assert.equal(config.wslIntegrationEnabled, false);
    const runtime = await browser.execute(() => ({
      userAgent: navigator.userAgent,
      platform: navigator.platform,
      scale: window.devicePixelRatio,
      width: innerWidth,
      height: innerHeight,
    }));
    await writeFile(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'runtime.json'), JSON.stringify(runtime, null, 2));
    await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'main.png'));
  });

  it('opens settings and renders light and dark themes at supported sizes', async () => {
    await $('header nav a[href="/settings"]').click();
    await $('button=English').waitForDisplayed();
    await $('button=English').click();
    for (const [width, height] of [[720, 520], [800, 600], [1000, 760]]) {
      // 原生窗口边框的计量因平台而异，根据 WebView 实测尺寸校正。
      let requestedWidth = width;
      let requestedHeight = height;
      for (let attempt = 0; attempt < 3; attempt++) {
        await browser.setWindowRect(null, null, requestedWidth, requestedHeight);
        await browser.executeAsync((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
        const actual = await browser.execute(() => ({ width: innerWidth, height: innerHeight }));
        if (actual.width === width && actual.height === height) break;
        requestedWidth += width - actual.width;
        requestedHeight += height - actual.height;
      }
      const viewport = await browser.execute(() => ({ width: innerWidth, height: innerHeight }));
      assert.deepEqual(viewport, { width, height });
      for (const theme of ['Light', 'Dark']) {
        await $(`button=${theme}`).click();
        await browser.waitUntil(async () => browser.execute((value) => document.documentElement.classList.contains(value), theme.toLowerCase()));
        const layout = await browser.execute(() => ({
          overflow: document.documentElement.scrollWidth > innerWidth,
          navigation: [...document.querySelectorAll('header nav a')].map((element) => {
            const rect = element.getBoundingClientRect();
            return rect.left >= 0 && rect.right <= innerWidth && rect.top >= 0 && rect.bottom <= innerHeight;
          }),
        }));
        assert.equal(layout.overflow, false);
        assert.ok(layout.navigation.length >= 4 && layout.navigation.every(Boolean));
        await $('header').moveTo();
        await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `settings-${width}x${height}-${theme.toLowerCase()}.png`));
      }
    }
    await $('header nav a[href="/"]').click();
    await browser.waitUntil(async () => browser.execute(() => location.pathname === '/'));
  });
});
