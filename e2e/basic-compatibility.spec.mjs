import assert from 'node:assert/strict';
import path from 'node:path';
import { browser, $ } from '@wdio/globals';

async function configure(width, language, dark, detail) {
  let outer = { width, height: 850 };
  for (let attempt = 0; attempt < 3; attempt++) {
    await browser.setWindowRect(null, null, outer.width, outer.height);
    const actual = await browser.execute(() => ({ width: innerWidth, height: innerHeight }));
    if (actual.width === width && actual.height === 850) break;
    outer.width += width - actual.width; outer.height += 850 - actual.height;
  }
  await browser.executeAsync((options, done) => window.configureBasicCompatibility(options).then(done), { language, dark, detail });
  await browser.executeAsync(async done => { await document.fonts.ready; requestAnimationFrame(() => requestAnimationFrame(done)); });
}

describe('Basic compatibility in the existing Skills workspace', () => {
  it('keeps download sources in the existing layout without failed-check summaries', async () => {
    await $('.skills-page-shell').waitForDisplayed();
    for (const width of [1280, 960, 760]) for (const detail of [false, true]) {
      const language = width === 760 ? 'en' : 'zh-CN';
      await configure(width, language, width === 760, detail);
      const observed = await browser.execute(() => ({
        width: innerWidth, overflow: document.documentElement.scrollWidth > innerWidth,
        text: document.body.innerText,
        sources: [...document.querySelectorAll('.skill-detail-metadata button')].map(node => node.textContent),
        clippedFilterLabels: [...document.querySelectorAll('.skills-page-shell button[aria-haspopup="listbox"] .truncate')].filter(node => node.scrollWidth > node.clientWidth + 1).map(node => node.textContent),
      }));
      assert.equal(observed.width, width);
      assert.equal(observed.overflow, false);
      assert.deepEqual(observed.clippedFilterLabels, []);
      assert.ok(observed.text.includes('docs.example.com'));
      assert.ok(!observed.text.includes('暂无法检测更新'));
      assert.ok(!observed.text.includes('could not be checked'));
      assert.ok(!observed.text.includes('暂不支持检查更新'));
      if (detail) assert.ok(observed.sources.some(text => text.includes('docs.example.com')));
      await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `compatibility-download-${width}-${detail ? 'detail' : 'cards'}.png`));
    }
  });

  it('discloses the safe URL and capability only on demand and filters downloads', async () => {
    await configure(1280, 'zh-CN', false, true);
    await $('.skill-detail-source').waitForDisplayed();
    await browser.execute(() => document.activeElement?.blur());
    await browser.execute(() => document.querySelector('.skill-detail-source').focus());
    assert.equal(await browser.execute(() => document.hasFocus()), true, 'native WebView must own keyboard focus');
    await browser.waitUntil(async () => Boolean(await $('.skill-detail-source').getAttribute('aria-describedby')));
    const tooltip = $('[data-slot="tooltip-content"][data-state="instant-open"]');
    await tooltip.waitForDisplayed();
    const text = await tooltip.getText();
    assert.ok(text.includes('https://docs.example.com/skills/code-review.zip'));
    assert.ok(text.includes('暂不支持检查更新'));
    assert.ok(!text.split('\n').includes('docs.example.com'));
    await $('button[aria-label="按来源筛选"]').click();
    await $('[role="option"][aria-label="docs.example.com (1)"]').click();
    const list = await browser.execute(() => document.querySelector('[id="skills-list-panel"]')?.innerText ?? document.querySelector('.skills-page-shell').innerText);
    assert.ok(list.includes('code-review'));
    assert.ok(!list.includes('frontend-design'));
  });
});
