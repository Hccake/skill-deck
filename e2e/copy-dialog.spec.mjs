import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { browser, $ } from '@wdio/globals';

async function settle() {
  await browser.executeAsync(async done => {
    await document.fonts.ready;
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await Promise.allSettled(document.getAnimations().filter(animation => animation.playState === 'running'
      && animation.effect?.getComputedTiming().iterations !== Infinity).map(animation => animation.finished));
    requestAnimationFrame(done);
  });
}

async function viewport(width, height) {
  let requestedWidth = width;
  let requestedHeight = height;
  for (let attempt = 0; attempt < 3; attempt++) {
    await browser.setWindowRect(null, null, requestedWidth, requestedHeight);
    await settle();
    const actual = await browser.execute(() => ({ width: innerWidth, height: innerHeight }));
    if (actual.width === width && actual.height === height) return;
    requestedWidth += width - actual.width;
    requestedHeight += height - actual.height;
  }
  assert.deepEqual(await browser.execute(() => [innerWidth, innerHeight]), [width, height]);
}

async function configure(options = {}) {
  await browser.executeAsync((options, done) => {
    window.configureCopyDialog({ language: 'zh-CN', dark: false, single: false, dependency: false, ...options }).then(done);
  }, options);
  await $('[role="dialog"]').waitForDisplayed();
  await $('[data-testid="copy-target-projects-scroll"] [role="checkbox"]').waitForDisplayed();
  await settle();
}

async function geometry() {
  return browser.execute(() => {
    const dialog = document.querySelector('[role="dialog"]');
    const projectsTitle = document.getElementById('copy-target-projects-title');
    const agentsTitle = document.getElementById('copy-agent-settings-title');
    const projects = projectsTitle.closest('section');
    const trigger = projects.querySelector('[role="combobox"]');
    const triggerLabel = trigger?.querySelector('[data-tooltip-label]');
    const footer = dialog.querySelector('[data-slot="dialog-footer"]');
    const rect = element => {
      if (!element) return null;
      const r = element.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height, centerY: r.top + r.height / 2 };
    };
    const projectScroll = dialog.querySelector('[data-testid="copy-target-projects-scroll"]');
    const agentScroll = dialog.querySelector('[data-testid="copy-agent-settings-scroll"]');
    // Agent 汇总条有用于对齐的负外边距；越界以实际面板和滚动容器为准。
    const overflowNodes = [dialog, projects, agentsTitle.closest('section'), projectScroll, agentScroll]
      .filter(element => element.scrollWidth > element.clientWidth + 1)
      .map(element => ({ tag: element.tagName, id: element.id, class: element.className,
        width: element.clientWidth, scrollWidth: element.scrollWidth, overflowX: getComputedStyle(element).overflowX }));
    return { window: [innerWidth, innerHeight], dialog: rect(dialog), projects: rect(projects),
      projectsTitle: rect(projectsTitle), agentsTitle: rect(agentsTitle), trigger: rect(trigger), footer: rect(footer),
      triggerTextClipped: triggerLabel ? triggerLabel.scrollWidth > triggerLabel.clientWidth : false,
      projectScroll: rect(projectScroll), agentScroll: rect(agentScroll),
      overflow: overflowNodes.length > 0, overflowNodes,
    };
  });
}

describe('Copy dialog with controlled data in the native WebView', () => {
  before(async () => {
    await $('[role="dialog"]').waitForDisplayed();
    const userAgent = await browser.execute(() => navigator.userAgent);
    assert.match(userAgent, { win32: /Windows/, linux: /Linux/, darwin: /Macintosh/ }[process.platform]);
  });

  it('aligns the system selector with the project title at supported window sizes', async () => {
    const records = [];
    for (const language of ['zh-CN', 'en']) {
      await configure({ language });
      for (const [width, height] of [[1040, 672], [800, 600], [720, 520]]) {
        await viewport(width, height);
        const state = await geometry();
        assert.ok(Math.abs(state.projectsTitle.centerY - state.trigger.centerY) < 2, JSON.stringify(state));
        assert.ok(Math.abs(state.projectsTitle.centerY - state.agentsTitle.centerY) < 2, JSON.stringify(state));
        assert.ok(state.trigger.left >= state.projectsTitle.right + 8, JSON.stringify(state));
        assert.ok(state.trigger.right <= state.projects.right, JSON.stringify(state));
        assert.ok(Math.abs(state.trigger.height - 32) < 1, JSON.stringify(state));
        assert.equal(state.triggerTextClipped, false, JSON.stringify(state));
        assert.ok(state.footer.bottom <= height && state.agentScroll.bottom <= state.footer.top, JSON.stringify(state));
        assert.ok(state.projectScroll.height >= 160, JSON.stringify(state));
        assert.equal(state.overflow, false, JSON.stringify(state));
        records.push({ language, ...state });
        if (language === 'zh-CN' && width !== 800) await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `copy-layout-${width}.png`));
      }
    }
    await writeFile(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'copy-layout.json'), JSON.stringify(records, null, 2));
  });

  it('shows platform icons and full menu names while keeping Agent choices on system changes', async () => {
    await configure({ dark: true });
    await viewport(800, 600);
    const trigger = $('[role="combobox"]');
    assert.equal(await trigger.$('[data-environment-platform="native"]').isExisting(), true);
    await $('[data-testid="copy-target-projects-scroll"] [role="checkbox"]').click();
    await trigger.click();
    const option = $('[role="option"][aria-label="Ubuntu-24.04-Development-Workspace"]');
    await option.waitForDisplayed();
    assert.equal(await option.$('[data-environment-platform="wsl"]').isExisting(), true);
    const menu = await browser.execute(() => {
      const content = document.querySelector('[role="listbox"]');
      const r = content.getBoundingClientRect();
      return { width: r.width, left: r.left, right: r.right, viewport: innerWidth, overflow: content.scrollWidth > content.clientWidth };
    });
    assert.ok(menu.width >= 240 && menu.left >= 0 && menu.right <= menu.viewport, JSON.stringify(menu));
    assert.equal(menu.overflow, false);
    await settle();
    await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'copy-system-menu-dark.png'));
    await option.click();
    await $('[data-testid="copy-target-projects-scroll"]').$('span=Linux skills').waitForDisplayed();
    assert.equal(await trigger.$('[data-environment-platform="wsl"]').isExisting(), true);
    assert.equal(await $('[role="checkbox"][aria-label="Claude Code"]').getAttribute('aria-checked'), 'true');
    assert.equal(await $('button=复制到 0 个项目').isEnabled(), false);
    await viewport(720, 520);
    assert.equal((await geometry()).overflow, false);
    await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'copy-long-system-dark.png'));
  });

  it('keeps the single-system dependency dialog readable at minimum size', async () => {
    await configure({ single: true, dependency: true });
    await viewport(720, 520);
    assert.equal(await $('[role="combobox"]').isExisting(), false);
    const state = await geometry();
    assert.equal(state.overflow, false);
    assert.ok(Math.abs(state.projectsTitle.centerY - state.agentsTitle.centerY) < 2, JSON.stringify(state));
    assert.ok(state.projectScroll.bottom <= state.footer.top, JSON.stringify(state));
    await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'copy-single-system-dependency.png'));
  });

  it('keeps English installation choices on one line without shrinking their icons', async () => {
    await configure({ language: 'en' });
    for (const [width, height] of [[1040, 672], [720, 520]]) {
      await viewport(width, height);
      const choices = await browser.execute(() => {
        const group = document.querySelector('[role="radiogroup"]');
        const groupBounds = group.getBoundingClientRect();
        return [...group.querySelectorAll('label')].map(label => {
          const bounds = label.getBoundingClientRect();
          const icon = [...label.children].find(child => child.tagName.toLowerCase() === 'svg');
          const walker = document.createTreeWalker(label, NodeFilter.SHOW_TEXT);
          const rects = [];
          while (walker.nextNode()) {
            if (!walker.currentNode.textContent.trim() || walker.currentNode.parentElement.closest('.sr-only')) continue;
            const range = document.createRange();
            range.selectNodeContents(walker.currentNode);
            rects.push(...[...range.getClientRects()].map(rect => ({ left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom })));
          }
          return { text: label.textContent, lines: rects.length, rects, iconWidth: icon.getBoundingClientRect().width,
            left: bounds.left, right: bounds.right, groupRight: groupBounds.right };
        });
      });
      for (const choice of choices) {
        assert.equal(choice.lines, 1, JSON.stringify({ width, choice }));
        assert.ok(Math.abs(choice.iconWidth - 14) < 1, JSON.stringify({ width, choice }));
        assert.ok(choice.rects.every(rect => rect.left >= choice.left && rect.right <= choice.right), JSON.stringify(choice));
        assert.ok(choice.right <= choice.groupRight, JSON.stringify(choice));
      }
      await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `copy-mode-english-${width}.png`));
    }
    const help = $('button[aria-label="Links share content; copies are independent."]');
    await browser.execute(() => document.querySelector('button[aria-label="Links share content; copies are independent."]').focus());
    const tooltip = $('[data-slot="tooltip-content"]');
    await tooltip.waitForDisplayed();
    assert.ok((await tooltip.getText()).includes('Links share content; copies are independent.'));
    assert.equal(await help.isFocused(), true);
    await settle();
    await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'copy-mode-help-english.png'));
  });
});
