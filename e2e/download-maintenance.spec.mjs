import assert from 'node:assert/strict';
import path from 'node:path';
import { browser, $ } from '@wdio/globals';

async function configure(options = {}, width = 960, height = 720) {
  let outer = { width, height };
  for (let attempt = 0; attempt < 3; attempt++) {
    await browser.setWindowRect(null, null, outer.width, outer.height);
    const actual = await browser.execute(() => ({ width: innerWidth, height: innerHeight }));
    if (actual.width === width && actual.height === height) break;
    outer.width += width - actual.width; outer.height += height - actual.height;
  }
  await browser.executeAsync((options, done) => window.configureDownloadMaintenance(options).then(done), options);
  if ((options.view ?? 'wizard') === 'wizard' && (options.step ?? 'confirm') === 'confirm') {
    await browser.waitUntil(async () => await browser.execute(count => window.__downloadFixture.lastPrepared.length === count, options.count ?? 20));
    await $('.install-confirmation').waitForDisplayed();
  } else await $(options.view === 'library' ? 'button[aria-label="Check for Updates"],button[aria-label="检查更新"]' : options.view === 'update' ? '[role="dialog"]' : '[data-e2e-wizard-footer]').waitForDisplayed();
  await browser.executeAsync(async done => {
    await document.fonts.ready;
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await Promise.allSettled(document.getAnimations().filter(animation => animation.effect?.getComputedTiming().iterations !== Infinity).map(animation => animation.finished));
    done();
  });
}

describe('Download maintenance confirmation in the native desktop', () => {
  it('keeps the four wizard steps in the same shell and footer', async () => {
    await $('[data-e2e-wizard-footer]').waitForDisplayed();
    for (const [width, height, language] of [[960, 720, 'zh-CN'], [720, 520, 'en']]) {
      let expected;
      for (const step of ['source', 'skills', 'options', 'confirm']) {
        await configure({ step, language, count: 2 }, width, height);
        const geometry = await browser.execute(() => {
          const footer = document.querySelector('[data-e2e-wizard-footer]').getBoundingClientRect();
          return { x: footer.x, y: footer.y, width: footer.width, height: footer.height, viewportHeight: innerHeight, overflow: document.documentElement.scrollWidth > innerWidth };
        });
        assert.equal(geometry.height, 72); assert.equal(geometry.y + geometry.height, height); assert.equal(geometry.overflow, false);
        if (expected) assert.deepEqual(geometry, expected); else expected = geometry;
      }
    }
  });

  it('keeps full installation scope while expanding Skills, locations and Agents', async () => {
    for (const [count, width, language, dark] of [[2, 960, 'zh-CN', false], [20, 960, 'zh-CN', false], [50, 720, 'en', true]]) {
      await configure({ count, language, dark, long: count === 50, oneTime: count === 50 }, width, 720);
      const names = await browser.execute(() => window.__downloadFixture.lastPrepared);
      const label = language === 'en' ? 'Allow installation' : '允许继续安装';
      const checkbox = $(`[role="checkbox"][aria-label="${label}"]`);
      await checkbox.waitForDisplayed();
      const install = $('[data-e2e-wizard-footer]').$(`button=${language === 'en' ? 'Install' : '安装'}`);
      assert.equal(await install.isEnabled(), false);
      const alignment = await browser.execute(() => {
        const checkbox = document.querySelector('[role="checkbox"]');
        const label = checkbox.parentElement.lastElementChild.getBoundingClientRect();
        const description = document.getElementById(checkbox.getAttribute('aria-describedby')).getBoundingClientRect();
        return { deltaX: description.x - label.x, gap: description.y - label.bottom, overflow: document.documentElement.scrollWidth > innerWidth };
      });
      assert.ok(Math.abs(alignment.deltaX) < 1); assert.equal(alignment.gap, 4); assert.equal(alignment.overflow, false);
      await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `download-confirm-${count}-${language}.png`));
      if (count > 3) {
        await $(`button=${language === 'en' ? `View all ${count} Skills` : `查看全部 ${count} 项`}`).click();
        assert.equal(await browser.execute(() => document.querySelectorAll('.install-skill-list li').length), count);
        await $(`button=${language === 'en' ? 'Show fewer Skills' : '收起清单'}`).click();
      }
      await $(`button=${language === 'en' ? 'View all 13 locations' : '查看全部 13 个位置'}`).click();
      assert.equal(await browser.execute(() => document.querySelectorAll('.install-target-list>li').length), 13);
      await $(`button=${language === 'en' ? 'Show fewer locations' : '收起位置'}`).click();
      await $(`button[aria-label="${language === 'en' ? 'View all 12 Agents' : '查看全部 12 个 Agent'}"]`).click();
      assert.ok((await $('.install-common-agent-details').getText()).includes('Team Assistant'));
      await checkbox.click();
      await browser.waitUntil(async () => install.isEnabled());
      assert.deepEqual(await browser.execute(() => window.__downloadFixture.lastPrepared), names);
    }
  });

  it('keeps a final-list conflict visible and blocks installation', async () => {
    await configure({ count: 20, conflict: true });
    assert.ok(await $('li*=team-skill-20').isDisplayed());
    await $('[role="checkbox"][aria-label="允许继续安装"]').click();
    assert.equal(await $('[data-e2e-wizard-footer]').$('button=安装').isEnabled(), false);
    assert.equal(await browser.execute(() => [...document.querySelectorAll('button')].some(button => button.textContent === '查看全部 13 个位置')), false);
  });

  it('shows the prepared source route inside the update dialog with initial focus on Cancel', async () => {
    await configure({ view: 'update', language: 'en', long: true }, 720, 520);
    const metrics = await browser.execute(() => {
      const dialog = document.querySelector('[role="dialog"]');
      const outer = dialog.getBoundingClientRect();
      return { focus: document.activeElement.textContent, tooltipCount: document.querySelectorAll('[role="tooltip"]').length, overflow: dialog.scrollWidth > dialog.clientWidth,
        buttonsInside: [...dialog.querySelectorAll('button')].every(button => button.getBoundingClientRect().right <= outer.right + 1), text: dialog.textContent };
    });
    assert.equal(metrics.focus, 'Cancel'); assert.equal(metrics.tooltipCount, 0); assert.equal(metrics.overflow, false); assert.equal(metrics.buttonsInside, true);
    assert.ok(metrics.text.includes('engineering-guidelines.downloads.example.com'));
    assert.ok(metrics.text.includes('content-distribution.engineering.example.net'));
    await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'download-update-source.png'));
  });
  it('keeps the library confirmation compact, preserves focus and reveals full hosts by keyboard', async () => {
    for (const [language, dark] of [['zh-CN', false], ['en', true]]) {
      await configure({ view: 'library', language, long: true, dark }, 760, 600);
      await $(`button[aria-label="${language === 'en' ? 'Check for Updates' : '检查更新'}"]`).click();
      const update = $('button[aria-label^="Update"][aria-label*="code-review"],button[aria-label^="更新"][aria-label*="code-review"]');
      await update.waitForDisplayed();
      await update.click();
      const dialog = $('[role="dialog"]');
      await dialog.waitForDisplayed();
      await browser.waitUntil(async () => await browser.execute(expected => document.activeElement.textContent === expected, language === 'en' ? 'Cancel' : '取消'));
      await browser.executeAsync(async done => {
        await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        await Promise.allSettled(document.getAnimations().filter(animation => animation.effect?.getComputedTiming().iterations !== Infinity).map(animation => animation.finished));
        done();
      });
      const metrics = await browser.execute(() => {
        const dialog = document.querySelector('[role="dialog"]');
        const rect = dialog.getBoundingClientRect();
        return { width: rect.width, overflow: dialog.scrollWidth > dialog.clientWidth, tooltipCount: document.querySelectorAll('[role="tooltip"]').length,
          bounded: [...dialog.querySelectorAll('button')].every(button => button.getBoundingClientRect().right <= rect.right + 1) };
      });
      assert.ok(metrics.width <= 512); assert.equal(metrics.overflow, false); assert.equal(metrics.tooltipCount, 0); assert.equal(metrics.bounded, true);
      await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `library-download-update-${language}.png`));
      const original = dialog.$('span[tabindex="0"]');
      await original.click();
      await browser.execute(() => document.querySelector('[role="dialog"] span[tabindex="0"]').focus());
      await $('[role="tooltip"]').waitForDisplayed();
      assert.ok((await $('[role="tooltip"]').getText()).includes('engineering-guidelines.downloads.example.com'));
      await browser.keys('Escape');
      await dialog.$(`button=${language === 'en' ? 'Cancel' : '取消'}`).click();
      await browser.waitUntil(async () => !(await dialog.isExisting()));
      assert.equal(await browser.execute(() => document.activeElement.getAttribute('aria-label')), language === 'en' ? 'Update "code-review"' : '更新「code-review」');
    }
  });

});
