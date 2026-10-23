import assert from 'node:assert/strict';
import path from 'node:path';
import { browser, $ } from '@wdio/globals';

async function settle() {
  await browser.executeAsync(async done => {
    await document.fonts.ready;
    requestAnimationFrame(() => requestAnimationFrame(done));
  });
}

async function viewport(width, height) {
  let requested = { width, height };
  for (let attempt = 0; attempt < 3; attempt++) {
    await browser.setWindowRect(null, null, requested.width, requested.height);
    await settle();
    const actual = await browser.execute(() => ({ width: innerWidth, height: innerHeight }));
    if (actual.width === width && actual.height === height) return;
    requested.width += width - actual.width;
    requested.height += height - actual.height;
  }
  assert.fail('Native viewport size did not converge');
}

async function configure(options = {}) {
  await browser.executeAsync((options, done) => window.configureUpdateRelocation({ language: 'zh-CN', long: false, library: false, dark: false, ...options }).then(done), options);
  await $('[role="dialog"] code').waitForDisplayed();
  await settle();
}

describe('Source path changes in the native update preview', () => {
  it('keeps names readable and old and new paths on one line in both preview widths', async () => {
    await $('[role="dialog"] code').waitForDisplayed();
    for (const library of [false, true]) for (const language of ['zh-CN', 'en']) for (const long of [false, true]) {
      await configure({ library, language, long, dark: library });
      await viewport(720, 520);
      const geometry = await browser.execute(() => {
        const row = document.querySelector('.update-skill-heading-row,[data-testid="library-path-preview"]');
        const heading = row.querySelector('h3');
        const codes = [...row.querySelectorAll('code')];
        const rect = element => { const r = element.getBoundingClientRect(); return { left: r.left, right: r.right, top: r.top, bottom: r.bottom }; };
        return { heading: rect(heading), paths: codes.map(rect), overflow: row.scrollWidth > row.clientWidth, footer: rect(document.querySelector('[data-slot="dialog-footer"]')), height: innerHeight };
      });
      assert.equal(geometry.overflow, false, JSON.stringify(geometry));
      assert.ok(Math.abs(geometry.paths[0].top - geometry.paths[1].top) < 1, JSON.stringify(geometry));
      assert.ok(geometry.footer.bottom <= geometry.height, JSON.stringify(geometry));
      if (!long) assert.ok(geometry.paths[0].top < geometry.heading.bottom && geometry.paths[0].left > geometry.heading.right, JSON.stringify(geometry));
      await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `relocation-${library ? 'library' : 'direct'}-${language}-${long ? 'long' : 'short'}.png`));
    }
  });

  it('retains existing fixed and optional copy choices', async () => {
    await configure();
    const fixed = $('[role="checkbox"][disabled]');
    assert.equal(await fixed.getAttribute('aria-checked'), 'true');
    const optional = $('[role="checkbox"]:not([disabled])');
    assert.equal(await optional.getAttribute('aria-checked'), 'false');
    await optional.click();
    assert.equal(await optional.getAttribute('aria-checked'), 'true');
    assert.equal(await fixed.getAttribute('aria-checked'), 'true');
  });

  it('reveals a truncated source path on keyboard focus', async () => {
    await configure({ long: true, language: 'en' });
    await viewport(720, 520);
    const value = await browser.execute(() => {
      const code = [...document.querySelectorAll('.update-skill-heading-row code')].find(element => element.scrollWidth > element.clientWidth);
      if (!code) return null;
      code.focus();
      return code.textContent;
    });
    assert.ok(value);
    const tooltip = $('[data-slot="tooltip-content"]');
    await tooltip.waitForDisplayed();
    // Tooltip 同时包含屏幕阅读器副本；这里只核对可见的路径节点。
    assert.equal(await tooltip.$(':scope > code').getText(), value);
  });
});
