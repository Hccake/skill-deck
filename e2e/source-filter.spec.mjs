import assert from 'node:assert/strict';
import path from 'node:path';
import { browser, $ } from '@wdio/globals';

async function settle() {
  await browser.executeAsync(async done => { await document.fonts.ready; requestAnimationFrame(() => requestAnimationFrame(done)); });
}
async function viewport() {
  let requested = { width: 720, height: 520 };
  for (let attempt = 0; attempt < 3; attempt++) {
    await browser.setWindowRect(null, null, requested.width, requested.height);
    await settle();
    const actual = await browser.execute(() => ({ width: innerWidth, height: innerHeight }));
    if (actual.width === 720 && actual.height === 520) return;
    requested.width += 720 - actual.width; requested.height += 520 - actual.height;
  }
  assert.fail('Native viewport did not converge');
}
async function configure(options = {}) {
  await browser.executeAsync((options, done) => window.configureSourceFilter({ width: 240, language: 'en', dark: false, long: false, ...options }).then(done), options);
  await settle();
}

async function closeMenu() {
  await browser.keys('Escape');
  await $('[data-slot="popover-content"]').waitForExist({ reverse: true });
  await settle();
}

describe('Native source filtering and Git references', () => {
  it('keeps both filters readable and their menus within the viewport at narrow list widths', async () => {
    await $('[data-testid="source-toolbar"]').waitForDisplayed();
    await viewport();
    for (const width of [240, 320, 700]) for (const language of ['zh-CN', 'en']) {
      await configure({ width, language, dark: language === 'en' });
      const geometry = await browser.execute(() => {
        const list = document.querySelector('[data-testid="source-list"]');
        const toolbar = document.querySelector('[data-testid="source-toolbar"]');
        const search = toolbar.querySelector('input[type="search"]').getBoundingClientRect();
        const filters = [...toolbar.querySelectorAll('button[aria-haspopup="listbox"]')].map(element => {
          const r = element.getBoundingClientRect(), label = element.querySelector('.truncate');
          return { top: r.top, left: r.left, right: r.right, labelClipped: label.scrollWidth > label.clientWidth };
        });
        return { width: innerWidth, overflow: list.scrollWidth > list.clientWidth, searchBottom: search.bottom, filters };
      });
      assert.equal(geometry.overflow, false, JSON.stringify(geometry));
      assert.equal(geometry.filters.length, 2);
      for (const filter of geometry.filters) assert.equal(filter.labelClipped, false, JSON.stringify(geometry));
      if (width <= 320) {
        assert.ok(geometry.filters[0].top >= geometry.searchBottom);
        assert.ok(Math.abs(geometry.filters[0].top - geometry.filters[1].top) < 1);
      }
      for (const index of [0, 1]) {
        const triggers = await $('[data-testid="source-toolbar"]').$$('button[aria-haspopup="listbox"]');
        await triggers[index].click();
        await $('[role="listbox"]').waitForDisplayed();
        await browser.executeAsync(async done => {
          await Promise.all(document.getAnimations().filter(animation => animation.effect?.getComputedTiming().iterations !== Infinity).map(animation => animation.finished.catch(() => {})));
          requestAnimationFrame(done);
        });
        const menu = await browser.execute(() => {
          const r = document.querySelector('[data-slot="popover-content"]').getBoundingClientRect();
          return { left: r.left, right: r.right, width: r.width, bottom: r.bottom, height: innerHeight, viewport: innerWidth };
        });
        assert.ok(menu.width >= 250 && menu.left >= 0 && menu.right <= menu.viewport && menu.bottom <= menu.height, JSON.stringify(menu));
        await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `source-filter-${width}-${language}-${index}.png`));
        await closeMenu();
      }
    }
  });

  it('selects a repository through the native popover and combines it with search', async () => {
    await configure();
    await $('button[aria-label="Filter by source"]').click();
    await $('[role="option"][aria-label="acme/tools (2)"]').click();
    assert.deepEqual(await browser.execute(() => [...document.querySelector('[data-testid="visible-skills"]').children].map(element => element.firstElementChild.textContent)), ['review', 'release']);
    await $('input[name="skill-search"]').setValue('review');
    assert.deepEqual(await browser.execute(() => [...document.querySelector('[data-testid="visible-skills"]').children].map(element => element.firstElementChild.textContent)), ['review']);
  });

  it('sizes the source menu to its content between 256 and 320 pixels', async () => {
    const widths = [];
    for (const [name, sourceLabel] of [
      ['short', 'mattpocock/skills'],
      ['medium', 'organization/platform-tools'],
      ['long', 'organization-with-long-name/repository-with-a-very-long-name'],
    ]) {
      await configure({ sourceLabel });
      await $('button[aria-label="Filter by source"]').click();
      await $('[role="listbox"]').waitForDisplayed();
      await browser.executeAsync(async done => {
        await Promise.all(document.getAnimations().filter(animation => animation.effect?.getComputedTiming().iterations !== Infinity).map(animation => animation.finished.catch(() => {})));
        requestAnimationFrame(done);
      });
      const geometry = await browser.execute(() => {
        const menu = document.querySelector('[data-slot="popover-content"]');
        const label = menu.querySelectorAll('[role="option"] .truncate')[1];
        return { width: menu.getBoundingClientRect().width, clipped: label.scrollWidth > label.clientWidth, overflow: menu.scrollWidth > menu.clientWidth };
      });
      widths.push(geometry.width);
      assert.ok(geometry.width >= 255 && geometry.width <= 321, JSON.stringify(geometry));
      assert.equal(geometry.overflow, false);
      assert.equal(geometry.clipped, name === 'long');
      await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `source-menu-width-${name}.png`));
      await closeMenu();
    }
    assert.ok(Math.abs(widths[0] - 256) <= 1, JSON.stringify(widths));
    assert.ok(widths[1] > widths[0] && widths[1] < widths[2], JSON.stringify(widths));
    assert.ok(Math.abs(widths[2] - 320) <= 1, JSON.stringify(widths));
  });

  it('discloses the complete pinned SHA and keeps long names within the list', async () => {
    await configure();
    await browser.execute(() => document.querySelector('[data-testid="reference-preview"] [tabindex="0"]').focus());
    const tooltip = $('[data-slot="tooltip-content"]');
    await tooltip.waitForDisplayed();
    assert.ok((await tooltip.getText()).includes('a1b2c3d4e5f678901234567890abcdef1234567890'));
    await configure({ long: true });
    const clipped = await browser.execute(() => {
      const row = document.querySelector('[data-testid="reference-preview"]');
      const code = row.querySelector('code');
      return row.scrollWidth <= row.clientWidth && code.scrollWidth > code.clientWidth;
    });
    assert.equal(clipped, true);
  });
});
