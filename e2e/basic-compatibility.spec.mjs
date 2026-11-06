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
  await browser.executeAsync(async done => {
    await document.fonts.ready;
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await Promise.allSettled(document.getAnimations().filter(animation => animation.effect?.getComputedTiming().iterations !== Infinity).map(animation => animation.finished));
    done();
  });
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
  it('keeps matching feedback inside both existing selection workflows', async () => {
    for (const mode of ['wizard', 'library']) {
      await configure(1280, 'zh-CN', false, false);
      await browser.execute(mode => window.setCompatibilityFlow(mode), mode);
      if (mode === 'library') {
        const input = $('input[name="library-add-source"]');
        await input.waitForDisplayed();
        await input.setValue('acme/team-skills#@code-review');
        await $('button=读取来源').click();
      }
      try {
        await $('input[name="source-skill-search"]').waitForDisplayed();
      } catch (error) {
        console.log('selection flow', mode, await browser.execute(() => document.body.innerText));
        throw error;
      }
      for (const width of [1280, 960, 760]) {
        await configure(width, width === 760 ? 'en' : 'zh-CN', width === 760, false);
        const state = await browser.execute(() => {
          const list = document.querySelector('[role="region"]');
          return {
            overflow: document.documentElement.scrollWidth > innerWidth,
            feedback: list.querySelector('[role="status"]')?.textContent,
            selected: [...list.querySelectorAll('[role="checkbox"]')].some(el => el.getAttribute('aria-checked') === 'true'),
            pathRows: [...list.querySelectorAll('label')].map(row => {
              const line = row.querySelector('[translate="no"]');
              return line && getComputedStyle(line).display === 'flex' && line.textContent.includes('SKILL.md') === false;
            }),
          };
        });
        assert.equal(await browser.execute(() => getComputedStyle(document.querySelector('[role="region"] label [translate="no"]')).color === getComputedStyle(document.body).color), true);
        assert.equal(state.overflow, false);
        assert.ok(state.feedback.includes('code-review') && state.feedback.includes('missing-skill'));
        assert.equal(state.selected, false);
        assert.deepEqual(state.pathRows, [true, true]);
        await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `compatibility-selection-${mode}-${width}.png`));
      }
      await $('[role="checkbox"][aria-label="code_review"]').click();
      const feedback = await $('[role="status"]').getText();
      assert.ok(!feedback.includes('code-review'));
      assert.ok(feedback.includes('missing-skill'));
    }
  });

  it('shows Antigravity current and compatible directories in the existing target step', async () => {
    await configure(1280, 'zh-CN', false, false);
    await browser.execute(() => window.setCompatibilityFlow('wizard'));
    await $('[role="checkbox"][aria-label="code_review"]').waitForDisplayed();
    await $('[role="checkbox"][aria-label="code_review"]').click();
    await $('button=下一步').click();
    await browser.waitUntil(async () => (await browser.execute(() => document.body.innerText)).includes('Antigravity IDE'));
    for (const width of [1280, 960, 760]) {
      await configure(width, width === 760 ? 'en' : 'zh-CN', width === 760, false);
      const state = await browser.execute(() => ({ overflow: document.documentElement.scrollWidth > innerWidth, text: document.body.innerText }));
      assert.equal(state.overflow, false);
      assert.ok(state.text.includes('Antigravity IDE'));
      assert.ok(state.text.includes('Antigravity CLI'));
      assert.ok(state.text.includes(width === 760 ? 'Compatibility directory' : '兼容目录'));
      await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `compatibility-antigravity-${width}.png`));
    }
  });

});
