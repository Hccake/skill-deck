import assert from 'node:assert/strict';
import path from 'node:path';
import { browser, $ } from '@wdio/globals';

async function settle() {
  await browser.executeAsync(async done => {
    await document.fonts.ready;
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await Promise.allSettled(document.getAnimations().filter(animation => animation.effect?.getComputedTiming().iterations !== Infinity).map(animation => animation.finished));
    done();
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
  it('allocates room to shared names and paths while keeping mixed states visible', async () => {
    for (const [width, locations, language, dark] of [
      [960, 'simple', 'zh-CN', false], [760, 'simple', 'en', false],
      [760, 'mixed', 'en', true], [720, 'long', 'zh-CN', true],
    ]) {
      await configure({ locations, language, dark });
      await viewport(width, 600);
      const geometry = await browser.execute(() => {
        const rect = el => { const r = el.getBoundingClientRect(); return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width }; };
        const body = document.querySelector('[data-testid="update-plan-dialog-body"]');
        return { overflow: body.scrollWidth > body.clientWidth, footer: rect(document.querySelector('[data-slot="dialog-footer"]')), height: innerHeight,
          rows: [...document.querySelectorAll('.update-position-row')].map(row => {
            const name = row.querySelector('.update-position-name'), path = row.querySelector('.update-position-path'), meta = row.querySelector('.update-position-metadata');
            return { row: rect(row), name: rect(name), path: rect(path), nameClipped: name.scrollWidth > name.clientWidth,
              meta: meta ? rect(meta) : null, metadataClipped: [...row.querySelectorAll('.update-position-status,.update-position-mode')].some(el => el.scrollWidth > el.clientWidth),
            };
          }),
        };
      });
      assert.equal(geometry.overflow, false);
      assert.ok(geometry.footer.bottom <= geometry.height);
      for (const row of geometry.rows) {
        assert.ok(Math.abs(row.name.left - geometry.rows[0].name.left) < 1);
        assert.ok(Math.abs(row.path.left - geometry.rows[0].path.left) < 1);
        assert.equal(row.metadataClipped, false);
        if (row.meta) assert.ok(row.meta.right <= row.row.right + 1);
        if (width >= 760) {
          assert.ok(row.name.width >= 180 && row.name.width <= 224);
          assert.ok(row.path.top < row.name.bottom);
          assert.equal(row.nameClipped, false);
        } else assert.ok(row.path.top >= row.name.bottom);
      }
      if (locations === 'simple') assert.equal(geometry.rows[0].meta, null);
      await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `update-columns-${locations}-${language}-${width}.png`));
    }
  });

  it('reveals clipped Agent names with keyboard focus without selecting a copy', async () => {
    await configure({ locations: 'long', language: 'en' });
    await viewport(760, 600);
    await browser.execute(() => document.activeElement?.blur());
    const label = await browser.execute(() => {
      const el = [...document.querySelectorAll('.update-position-name')].find(el => el.scrollWidth > el.clientWidth);
      if (!el) return null;
      el.focus(); return el.textContent;
    });
    assert.ok(label?.includes('Antigravity IDE') && label.includes('Antigravity CLI'));
    const tooltip = $('[data-slot="tooltip-content"]');
    await tooltip.waitForDisplayed();
    assert.ok((await tooltip.getText()).includes(label));
    assert.equal(await $('[role="checkbox"]:not([disabled])').getAttribute('aria-checked'), 'false');
  });

});
