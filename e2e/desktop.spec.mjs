import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { browser, $ } from '@wdio/globals';

async function settle() {
  await browser.executeAsync(async done => {
    await document.fonts.ready;
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await Promise.allSettled(document.getAnimations().filter(animation => animation.playState === 'running' && animation.effect?.getComputedTiming().iterations !== Infinity).map(animation => animation.finished));
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
  assert.deepEqual(await browser.execute(() => ({ width: innerWidth, height: innerHeight })), { width, height });
}

async function invoke(command, args) {
  const result = await browser.executeAsync((command, args, done) => {
    window.__TAURI_INTERNALS__.invoke(command, args).then(value => done({ value }), error => done({ error }));
  }, command, args);
  assert.equal(result.error, undefined, `${command}: ${JSON.stringify(result.error)}`);
  return result.value;
}

async function panelWidths(kind) {
  return browser.execute(kind => {
    const list = document.getElementById(kind === 'skills' ? 'skills-list-panel' : 'library-skills-list-panel');
    const detail = document.getElementById(kind === 'skills' ? 'skill-detail-panel' : 'library-skill-detail-panel');
    return { list: list.getBoundingClientRect().width, detail: detail.getBoundingClientRect().width };
  }, kind);
}

async function assertAutomaticSplit(kind) {
  for (const [width, list, detail] of [[720, 240, 479], [800, 240, 559], [900, 269.7, 629.3], [1000, 240, 551], [1280, 320, 751]]) {
    await viewport(width, 600);
    const actual = await panelWidths(kind);
    assert.ok(Math.abs(actual.list - list) < 2 && Math.abs(actual.detail - detail) < 2, JSON.stringify({ kind, width, actual }));
    assert.equal(await $(`#${kind}-context-navigation`).isDisplayed(), width >= 1000);
    if (width <= 900) await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `workspace-${kind}-default-${width}.png`));
  }
  const preferred = await browser.execute(kind => {
    const state = JSON.parse(localStorage.getItem('skill-deck-layout'))?.state;
    return state?.[kind === 'skills' ? 'skillsListWidth' : 'libraryListWidth'] ?? null;
  }, kind);
  assert.equal(preferred, null, 'Automatic window resizing must not create a manual width preference');
}

async function resizeSeparator(key, kind = 'skills', part = 'detail') {
  // embedded 1.4 的拖动仅派发 MouseEvent；PointerEvent 鼠标拖动由 Windows 原生补验。
  await browser.execute(id => document.getElementById(id).focus(), `${kind}-${part}-resize`);
  await browser.keys(key);
  await settle();
}

async function assertDetailHeader(kind) {
  const geometry = await browser.execute(kind => {
    const panel = document.getElementById(kind === 'skills' ? 'skill-detail-panel' : 'library-skill-detail-panel');
    const title = panel.querySelector('h2');
    const row = title.parentElement.parentElement;
    const actions = row.lastElementChild;
    const rect = element => {
      const bounds = element.getBoundingClientRect();
      return { x: bounds.x, y: bounds.y, right: bounds.right, width: bounds.width };
    };
    const text = document.createRange();
    text.selectNodeContents(title);
    return { row: rect(row), title: rect(title), textWidth: text.getBoundingClientRect().width, actions: rect(actions), gap: parseFloat(getComputedStyle(row).columnGap) };
  }, kind);
  assert.ok(geometry.textWidth + geometry.actions.width + geometry.gap <= geometry.row.width, `Short-title fixture must fit: ${JSON.stringify(geometry)}`);
  assert.ok(Math.abs(geometry.title.y - geometry.actions.y) < 2, `Title and actions should share a row: ${JSON.stringify(geometry)}`);
  assert.ok(geometry.actions.right <= geometry.row.right + 1, JSON.stringify(geometry));
}

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
      await viewport(width, height);
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
        const links = await browser.execute(() => [...document.querySelectorAll('header nav a')].map(link => ({
          height: link.getBoundingClientRect().height,
          labelVisible: link.querySelector('span').getBoundingClientRect().width > 0,
        })));
        assert.ok(links.every(link => Math.abs(link.height - 32) < 1), JSON.stringify(links));
        if (width >= 800) assert.ok(links.every(link => link.labelVisible), JSON.stringify(links));
        await $('header').moveTo();
        await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `settings-${width}x${height}-${theme.toLowerCase()}.png`));
      }
    }
    await $('header nav a[href="/"]').click();
    await browser.waitUntil(async () => browser.execute(() => location.pathname === '/'));
  });

  it('adapts navigation labels to the space left by surrounding controls', async () => {
    await viewport(800, 600);
    const labels = () => browser.execute(() => [...document.querySelectorAll('header nav a > span')].filter(el => el.getBoundingClientRect().width > 0).length);
    assert.equal(await labels(), 4);
    try {
      // 占宽夹具只验证导航容量，不代表真实 WSL 切换。
      await browser.execute(() => { document.querySelector('header').firstElementChild.style.width = '280px'; });
      await browser.waitUntil(async () => (await labels()) === 1);
      assert.equal(await $('header nav a[aria-current="page"] > span').isDisplayed(), true);
      const heights = await browser.execute(() => [...document.querySelectorAll('header nav a')].map(el => el.getBoundingClientRect().height));
      assert.ok(heights.every(height => Math.abs(height - 32) < 1), JSON.stringify(heights));
    } finally {
      await browser.execute(() => { document.querySelector('header').firstElementChild.style.removeProperty('width'); });
    }
    await browser.waitUntil(async () => (await labels()) === 4);
  });

  it('preserves user panel widths and navigation focus in the real workspaces', async () => {
    const home = process.env.SKILL_DECK_E2E_HOME;
    const fixture = path.join(home, '.agents', 'skills', 'review');
    await mkdir(fixture, { recursive: true });
    await writeFile(path.join(fixture, 'SKILL.md'), '---\nname: review\ndescription: Desktop workspace validation.\n---\n# Workspace review\n\nVerify navigation, resizing, and keyboard focus.\n');
    for (const [name, description] of [
      ['code-review', 'Review code changes for correctness, maintainability, and security.'],
      ['cross-platform-testing-and-validation', 'Validate paths, file operations, and application behavior on Windows, macOS, and Linux. Review platform-specific assumptions, record evidence from each runtime, and confirm that the final package contains the expected resources.'],
      ['frontend-design', 'Create consistent interfaces for desktop applications and web products.'],
    ]) {
      const directory = path.join(home, '.agents', 'skills', name);
      await mkdir(directory, { recursive: true });
      await writeFile(path.join(directory, 'SKILL.md'), `---\nname: ${name}\ndescription: ${description}\n---\n# ${name}\n\n${description}\n`);
    }
    const environment = { kind: 'native' };
    const catalog = await invoke('create_skill_library', { environment, name: 'Workspace review' });
    const libraryId = catalog.libraries.find(library => library.name === 'Workspace review').id;
    const source = await invoke('discover_skill_source', { environment, source: fixture, operationId: 'workspace-layout', selectionIntent: { wildcardRequested: true, explicitSkillNames: [] } });
    const payloads = await invoke('acquire_selected_payloads', { request: { discoverySession: source.discoverySession, skillPaths: source.skills.map(skill => skill.relativePath) } });
    const request = { environment, libraryId, discoverySession: source.discoverySession, skills: source.skills.map(skill => ({ skillName: skill.installDirName, payload: payloads.find(payload => payload.skillPath === skill.relativePath) })) };
    const preview = await invoke('preview_add_library_skills', { request });
    await invoke('add_skills_to_library', { request: { request, expectedToken: preview.token, membership: preview.membership, acknowledgeRedirect: false } });
    await browser.refresh();
    await $('[data-testid="skill-card-title"] button').waitForDisplayed();
    await viewport(800, 600);
    const cards = await browser.execute(() => [...document.querySelectorAll('[data-slot="card"]')]
      .filter(card => card.querySelector('[data-testid="skill-card-title"]'))
      .map(card => {
        const source = card.querySelector('[data-testid="skill-card-meta"]').getBoundingClientRect();
        const agent = [...card.querySelectorAll('span')].find(el => el.textContent === 'Codex').getBoundingClientRect();
        const title = card.querySelector('h3').getBoundingClientRect();
        const description = card.querySelector('p').getBoundingClientRect();
        const action = card.querySelector('[data-testid="skill-card-title"]').nextElementSibling.querySelector('button').getBoundingClientRect();
        return { height: card.getBoundingClientRect().height, bottom: card.getBoundingClientRect().bottom, sourceX: source.x, sourceBottom: source.bottom, agentX: agent.x, agentY: agent.y, agentHeight: agent.height, titleX: title.x, descriptionX: description.x, titleDescriptionGap: description.y - title.bottom, descriptionSourceGap: source.y - description.bottom, actionHeight: action.height, actionDescriptionGap: description.y - action.bottom };
      }));
    assert.equal(cards.length, 4);
    assert.ok(cards.every(card => Math.abs(card.agentY - card.sourceBottom - 8) < 1 && Math.abs(card.agentX - card.sourceX) < 1), JSON.stringify(cards));
    assert.ok(cards.every(card => Math.abs(card.titleX - card.sourceX) < 1 && Math.abs(card.descriptionX - card.sourceX) < 1 && Math.abs(card.agentHeight - 20) < 1), JSON.stringify(cards));
    assert.ok(cards.every(card => Math.abs(card.titleDescriptionGap - 4) < 1 && Math.abs(card.descriptionSourceGap - 6) < 1 && Math.abs(card.actionHeight - 28) < 1 && card.actionDescriptionGap >= -0.5), JSON.stringify(cards));
    assert.ok(cards.every(card => card.height <= 134) && cards.filter(card => card.bottom <= 600).length >= 3, JSON.stringify(cards));
    await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'workspace-card-density-800.png'));
    const title = '[data-testid="skill-card-title"] button';
    await browser.execute(selector => document.querySelector(selector).focus(), title);
    await $('[role="tooltip"]').waitForExist();
    await browser.executeAsync(done => {
      document.querySelector('input[name="skill-search"]').focus();
      requestAnimationFrame(() => requestAnimationFrame(done));
    });
    await browser.execute(selector => document.querySelector(selector).focus(), title);
    // 等退出动画结束后再确认，避免只观察到一个瞬间出现的旧提示。
    await browser.executeAsync(done => setTimeout(done, 250));
    const disclosure = await browser.execute(selector => ({
      name: document.querySelector(selector).textContent,
      tooltip: document.querySelector('[role="tooltip"]')?.textContent,
    }), title);
    assert.equal(disclosure.tooltip, disclosure.name);
    await browser.execute(() => document.activeElement.blur());
    await viewport(1280, 800);
    await $('[data-testid="skill-card-title"] button').click();
    await $('.skill-prose').waitForDisplayed();
    await settle();
    await assertAutomaticSplit('skills');
    assert.ok(Math.abs((await panelWidths('skills')).list - 320) < 2);
    await resizeSeparator('ArrowRight');
    const chosen = (await panelWidths('skills')).list;
    assert.ok(chosen > 370 && chosen < 430, `Keyboard resize result: ${chosen}`);

    await viewport(720, 520);
    const narrow = await panelWidths('skills');
    assert.ok(Math.abs(narrow.detail - 360) < 2, JSON.stringify(narrow));
    assert.ok(narrow.list >= 239 && narrow.detail >= 359, JSON.stringify(narrow));
    await assertDetailHeader('skills');
    assert.equal(await $('#skills-context-navigation').isDisplayed(), false);
    assert.equal(await $('header nav a[aria-current="page"] span').isDisplayed(), true);
    await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'workspace-skills-720.png'));
    // 原生窗口可能仍位于系统鼠标下方；先关闭悬停提示，再单独验证抽屉的 Esc。
    if (await $('[role="tooltip"]').isExisting()) {
      await browser.keys('Escape');
      await $('[role="tooltip"]').waitForExist({ reverse: true });
    }
    const toggle = $('[aria-controls="skills-context-navigation"]');
    await toggle.click();
    assert.equal(await browser.execute(() => document.activeElement.getAttribute('aria-current')), 'location');
    await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'workspace-navigation-720.png'));
    await browser.keys('Escape');
    assert.equal(await toggle.getAttribute('aria-expanded'), 'false');
    assert.equal(await browser.execute(() => document.activeElement.getAttribute('aria-controls')), 'skills-context-navigation');
    assert.equal(await $('.skill-prose').isDisplayed(), true);

    await viewport(1280, 800);
    assert.ok(Math.abs((await panelWidths('skills')).list - chosen) < 2);
    await browser.refresh();
    await $('[data-testid="skill-card-title"] button').waitForDisplayed();
    await $('[data-testid="skill-card-title"] button').click();
    await $('.skill-prose').waitForDisplayed();
    await settle();
    assert.ok(Math.abs((await panelWidths('skills')).list - chosen) < 2);

    await $('header nav a[href="/libraries"]').click();
    await $('[data-testid="library-skill-title"] button').waitForDisplayed();
    const libraryCard = await browser.execute(() => {
      const card = document.querySelector('[data-testid="library-skill-title"]').closest('[data-slot="card"]');
      const [title, description, source] = ['h3', 'p', '[data-testid="skill-card-meta"]'].map(selector => card.querySelector(selector).getBoundingClientRect());
      return { lefts: [title.x, description.x, source.x], titleDescriptionGap: description.y - title.bottom, descriptionSourceGap: source.y - description.bottom };
    });
    assert.ok(libraryCard.lefts.every(left => Math.abs(left - libraryCard.lefts[0]) < 1), JSON.stringify(libraryCard));
    assert.ok(Math.abs(libraryCard.titleDescriptionGap - 4) < 1 && Math.abs(libraryCard.descriptionSourceGap - 6) < 1, JSON.stringify(libraryCard));
    await $('[data-testid="library-skill-title"] button').click();
    await $('.skill-prose').waitForDisplayed();
    await settle();
    await assertAutomaticSplit('library');
    assert.ok(Math.abs((await panelWidths('library')).list - 320) < 2);
    await resizeSeparator('ArrowLeft', 'library');
    const libraryWidth = (await panelWidths('library')).list;
    assert.ok(libraryWidth > 250 && libraryWidth < 290, `Library resize result: ${libraryWidth}`);
    await viewport(800, 600);
    const libraryNarrow = await panelWidths('library');
    assert.ok(libraryNarrow.list >= 239 && libraryNarrow.detail >= 359, JSON.stringify(libraryNarrow));
    await assertDetailHeader('library');
    await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'workspace-library-800.png'));
    await viewport(720, 520);
    assert.equal(await $('#library-context-navigation').isDisplayed(), false);
    await $('[aria-controls="library-context-navigation"]').click();
    await browser.keys('Escape');
    assert.equal(await $('.skill-prose').isDisplayed(), true);
    await viewport(1280, 800);
    const restoredLibraryWidth = (await panelWidths('library')).list;
    const preferences = await browser.execute(() => localStorage.getItem('skill-deck-layout'));
    await writeFile(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'workspace-widths.json'), JSON.stringify({ chosen, libraryWidth, restoredLibraryWidth, preferences }, null, 2));
    assert.ok(Math.abs(restoredLibraryWidth - libraryWidth) < 2, JSON.stringify({ libraryWidth, restoredLibraryWidth, preferences }));
    await $('header nav a[href="/"]').click();
    await $('.skill-prose').waitForDisplayed();
    await settle();
    assert.ok(Math.abs((await panelWidths('skills')).list - chosen) < 2);
    await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'workspace-skills-restored.png'));
  });

  it('keeps split panes while switching through narrow navigation and uses page-owned headers', async () => {
    const environment = { kind: 'native' };
    const project = path.join(process.env.SKILL_DECK_E2E_HOME, 'projects', 'scope-layout');
    await mkdir(project, { recursive: true });
    await invoke('add_environment_project', { environment, nativePath: project });
    await invoke('create_skill_library', { environment, name: 'Empty workspace' });
    await browser.refresh();
    await viewport(720, 520);
    await $('[data-testid="skill-card-title"] button').waitForDisplayed();
    await $('[data-testid="skill-card-title"] button').click();
    await $('.skill-prose').waitForDisplayed();
    const skillsWidth = (await panelWidths('skills')).list;
    assert.equal(await browser.execute(() => document.getElementById('skills-list-panel').contains(document.querySelector('[aria-controls="skills-context-navigation"]'))), true);
    await $('[aria-controls="skills-context-navigation"]').click();
    await $('#skills-context-navigation [data-project-id] > button').click();
    await browser.waitUntil(async () => (await $('#skill-detail-panel').getText()).includes('No Skill selected'));
    assert.equal(await $('#skills-context-navigation').isDisplayed(), false);
    assert.ok(Math.abs((await panelWidths('skills')).list - skillsWidth) < 2);
    await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'workspace-scope-switch-empty.png'));
    await $('#skills-list-panel').$('button*=review').click();
    await $('.skill-prose').waitForDisplayed();
    await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'workspace-scope-switch-selected.png'));

    await $('header nav a[href="/libraries"]').click();
    await $('#library-context-navigation').waitForExist();
    if (!(await $('#library-context-navigation').isDisplayed())) await $('[aria-controls="library-context-navigation"]').click();
    await $('#library-context-navigation').$('button*=Workspace review').click();
    await $('#library-skills-list-panel').$('button*=review').click();
    await $('.skill-prose').waitForDisplayed();
    const libraryWidth = (await panelWidths('library')).list;
    assert.equal(await browser.execute(() => document.querySelector('.library-workspace-header').contains(document.querySelector('[aria-controls="library-context-navigation"]'))), true);
    await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'workspace-library-single-header.png'));
    await $('[aria-controls="library-context-navigation"]').click();
    await $('#library-context-navigation').$('button*=Empty workspace').click();
    await browser.waitUntil(async () => (await $('#library-skill-detail-panel').getText()).includes('No Skill selected'));
    assert.equal(await $('#library-context-navigation').isDisplayed(), false);
    assert.ok(Math.abs((await panelWidths('library')).list - libraryWidth) < 2);
    await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'workspace-library-switch-empty.png'));
    await $('#library-skill-detail-panel button[aria-label="Close"]').click();
    assert.equal(await $('#library-skill-detail-panel').isExisting(), false);
    assert.equal(await $('#library-context-navigation').isDisplayed(), true);
    await $('header nav a[href="/"]').click();
  });

  it('keeps navigation and window controls separate when both status entries are visible', async () => {
    const main = await browser.getWindowHandle();
    await invoke('open_install_wizard', {
      entryPoint: 'skills-panel',
      context: { environment: { kind: 'native' }, scope: { scope: 'global' } },
      projectPath: null, prefillSource: null, prefillSkillName: null,
    });
    await browser.waitUntil(async () => (await browser.getWindowHandles()).length === 2);
    const handles = await browser.getWindowHandles();
    await browser.switchToWindow(main);
    await viewport(720, 520);
    await $('header button[aria-label="View issues requiring attention"]').waitForDisplayed();
    await $('header button[aria-label="Changes temporarily unavailable"]').waitForDisplayed();
    const overlap = await browser.execute(() => {
      const controls = [...document.querySelectorAll('header nav a,header button')]
        .filter(el => el.getClientRects().length).map(el => el.getBoundingClientRect());
      return controls.some((a, i) => controls.slice(i + 1).some(b => (
        Math.min(a.right, b.right) - Math.max(a.left, b.left) > 1
        && Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top) > 1
      )));
    });
    assert.equal(overlap, false);
    await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'workspace-header-status-720.png'));
    await browser.switchToWindow(handles.find(handle => handle !== main));
    await $('button=Cancel').waitForDisplayed();
    await $('button=Cancel').click();
    await browser.switchToWindow(main);
  });

  it('resizes the two sidebars independently and keeps drawer contents mounted', async () => {
    const widthOf = kind => browser.execute(kind => document.getElementById(`${kind}-context-navigation`).getBoundingClientRect().width, kind);
    for (const [kind, route, title, detailId, key, steps, expected] of [
      ['skills', '/', '[data-testid="skill-card-title"] button', 'skill-detail-panel', 'ArrowRight', 2, 320],
      ['library', '/libraries', '[data-testid="library-skill-title"] button', 'library-skill-detail-panel', 'ArrowLeft', 1, 192],
    ]) {
      await viewport(1280, 800);
      await $(`header nav a[href="${route}"]`).click();
      if (kind === 'skills') await $('#skills-context-navigation aside > div:first-child button').click();
      else await $('#library-context-navigation').$('button*=Workspace review').click();
      if (!(await $(`#${detailId}`).isExisting())) {
        await $(title).waitForDisplayed();
        await $(title).click();
      }
      await settle();
      assert.ok(Math.abs(await widthOf(kind) - 208) < 2);
      // embedded 1.4 未转换 Home/End 的 WebDriver 键码；边界通过方向键到达。
      for (let step = 0; step < steps; step++) await resizeSeparator(key, kind, 'sidebar');
      const sidebarState = await browser.execute(kind => ({
        width: document.getElementById(`${kind}-context-navigation`).getBoundingClientRect().width,
        panelWidth: document.getElementById(`${kind}-navigation-panel`).getBoundingClientRect().width,
        active: document.activeElement.id,
        preferences: localStorage.getItem('skill-deck-layout'),
      }), kind);
      assert.ok(Math.abs(sidebarState.width - expected) < 2, JSON.stringify({ kind, expected, ...sidebarState }));
      await browser.execute(kind => { window.__sidebarBeforeResize = document.getElementById(`${kind}-context-navigation`); }, kind);
      await viewport(720, 520);
      assert.equal(await $(`#${kind}-sidebar-resize`).isDisplayed(), false);
      assert.equal(await browser.execute(() => document.activeElement.getAttribute('aria-controls')), `${kind}-context-navigation`);
      await $(`[aria-controls="${kind}-context-navigation"]`).click();
      assert.ok(Math.abs(await widthOf(kind) - 208) < 2, 'Drawers keep a stable width');
      assert.equal(await browser.execute(kind => window.__sidebarBeforeResize === document.getElementById(`${kind}-context-navigation`), kind), true);
      await browser.keys('Escape');
      await viewport(1280, 800);
      assert.ok(Math.abs(await widthOf(kind) - expected) < 2);
      await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `workspace-${kind}-sidebar-resized.png`));
    }
    await viewport(1000, 760);
    await resizeSeparator('ArrowRight', 'library', 'sidebar');
    await resizeSeparator('ArrowRight', 'library', 'sidebar');
    assert.equal(await $('#library-context-navigation').isDisplayed(), true);
    assert.ok(Math.abs(await widthOf('library') - 279) < 2);
    const panels = await panelWidths('library');
    assert.ok(panels.list >= 239 && panels.detail >= 359, JSON.stringify(panels));
    await viewport(1280, 800);
    await browser.refresh();
    await $('header nav a[href="/libraries"]').waitForDisplayed();
    await $('header nav a[href="/libraries"]').click();
    await settle();
    assert.ok(Math.abs(await widthOf('library') - 279) < 2);
    await $('header nav a[href="/"]').click();
    await $('#skills-context-navigation').waitForDisplayed();
    await settle();
    assert.ok(Math.abs(await widthOf('skills') - 320) < 2);
    await browser.execute(() => { delete window.__sidebarBeforeResize; });
  });
  it('navigates scoped Skills and library members with separate focus and selection', async () => {
    const environment = { kind: 'native' };
    const home = process.env.SKILL_DECK_E2E_HOME;
    const projectSkill = path.join(home, 'projects', 'scope-layout', '.agents', 'skills', 'review');
    await mkdir(projectSkill, { recursive: true });
    await writeFile(path.join(projectSkill, 'SKILL.md'), '---\nname: review\ndescription: Project keyboard fixture.\n---\n# Project review\n');
    const catalog = await invoke('list_skill_libraries', { environment });
    const libraryId = catalog.libraries.find(library => library.name === 'Workspace review').id;
    const source = await invoke('discover_skill_source', { environment, source: path.join(home, '.agents', 'skills', 'code-review'), operationId: 'keyboard-review', selectionIntent: { wildcardRequested: true, explicitSkillNames: [] } });
    const payloads = await invoke('acquire_selected_payloads', { request: { discoverySession: source.discoverySession, skillPaths: source.skills.map(skill => skill.relativePath) } });
    const request = { environment, libraryId, discoverySession: source.discoverySession, skills: source.skills.map(skill => ({ skillName: skill.installDirName, payload: payloads.find(payload => payload.skillPath === skill.relativePath) })) };
    const preview = await invoke('preview_add_library_skills', { request });
    await invoke('add_skills_to_library', { request: { request, expectedToken: preview.token, membership: preview.membership, acknowledgeRedirect: false } });
    await browser.refresh();
    await viewport(1280, 800);

    for (const [kind, route, listId, detailId] of [
      ['skills', '/', 'skills-list-panel', 'skill-detail-panel'],
      ['library', '/libraries', 'library-skills-list-panel', 'library-skill-detail-panel'],
    ]) {
      await $(`header nav a[href="${route}"]`).click();
      await settle();
      if (kind === 'library') await $('#library-context-navigation').$('button*=Workspace review').click();
      else await $('#skills-context-navigation [data-project-id] > button').click();
      await settle();
      if (await $(`#${detailId}`).isExisting()) await $(`#${detailId} button[title="Close"], #${detailId} button[aria-label="Close"]`).click();
      const first = `#${listId} button[data-skill-open]`;
      await $(first).waitForDisplayed();
      await browser.execute(selector => document.querySelector(selector).focus(), first);
      // embedded 1.4 仅合成 KeyboardEvent，不执行按钮的默认激活；Enter/Space 由受信输入补验。
      await $(first).click();
      await $('.skill-prose').waitForDisplayed();
      const state = () => browser.execute((listId, detailId) => {
        const rows = [...document.getElementById(listId).querySelectorAll('button[data-skill-open]')];
        return {
          active: rows.indexOf(document.activeElement),
          selected: rows.findIndex(row => row.getAttribute('aria-current') === 'true'),
          tabStops: rows.map(row => row.tabIndex),
          keys: rows.map(row => row.closest('[data-skill-item]').dataset.skillItem),
          detail: document.getElementById(detailId)?.querySelector('h2')?.textContent,
        };
      }, listId, detailId);
      const before = await state();
      assert.equal(before.active, 0, `${kind}: title should hand focus to the corresponding row`);
      assert.equal(before.tabStops.filter(value => value === 0).length, 1);
      if (kind === 'skills') assert.ok(before.keys.some(key => key === 'global:review') && before.keys.some(key => key.startsWith('project:') && key.endsWith(':review')));
      await browser.keys('ArrowDown');
      const moved = await state();
      assert.equal(moved.active, 1);
      assert.equal(moved.selected, 0);
      assert.equal(moved.detail, before.detail);
      await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `keyboard-${kind}-focus-selection.png`));
      await browser.execute(() => document.activeElement.click());
      await browser.waitUntil(async () => (await state()).selected === 1);
      await browser.keys(['Control', 'f']);
      assert.equal(await browser.execute(() => document.activeElement.hasAttribute('data-skill-search')), true);
      const search = $(`#${listId} input[data-skill-search]`);
      await search.setValue('no-matching-skill-keyboard');
      await browser.keys('Escape');
      assert.equal(await search.getValue(), '');
      assert.equal(await $(`#${detailId}`).isExisting(), true);
      assert.equal(await browser.execute(() => document.activeElement.hasAttribute('data-skill-search')), true);
      const selectedKey = (await state()).keys[1];
      await browser.keys('Escape');
      assert.equal(await $(`#${detailId}`).isExisting(), false);
      assert.equal(await browser.execute(() => document.activeElement.closest('[data-skill-item]')?.dataset.skillItem), selectedKey);
      await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `keyboard-${kind}-return.png`));
    }
  });
  it('adapts both detail metadata layouts to the panel width inside a wide window', async () => {
    const saved = await browser.execute(() => localStorage.getItem('skill-deck-layout'));
    try {
      await viewport(1280, 800);
      for (const [kind, route, panelId, titleSelector] of [
        ['skills', '/', 'skill-detail-panel', '[data-testid="skill-card-title"] button'],
        ['library', '/libraries', 'library-skill-detail-panel', '[data-testid="library-skill-title"] button'],
      ]) {
      for (const width of [360, 480, 640]) {
        await browser.execute(({ saved, width, kind }) => {
          const layout = JSON.parse(saved);
          Object.assign(layout.state, { skillsSidebarWidth: 208, librarySidebarWidth: 208, skillsDetailOpen: false, libraryDetailOpen: false });
          layout.state[kind === 'skills' ? 'skillsListWidth' : 'libraryListWidth'] = 1071 - width;
          localStorage.setItem('skill-deck-layout', JSON.stringify(layout));
        }, { saved, width, kind });
        await browser.refresh();
        await $(`header nav a[href="${route}"]`).waitForDisplayed();
        await $(`header nav a[href="${route}"]`).click();
        await $(titleSelector).waitForDisplayed();
        await $(titleSelector).click();
        await $('.skill-prose').waitForDisplayed(); await settle();
        const geometry = await browser.execute(panelId => {
          const panel = document.getElementById(panelId);
          const content = panel.querySelector('.skill-detail-content');
          const fields = [...panel.querySelectorAll('.skill-detail-field')];
          const path = panel.querySelector('button[aria-label="Copy path"]')?.closest('.skill-detail-field');
          const agentField = panel.querySelector('.skill-agent-badge')?.closest('.skill-detail-field');
          return {
            width: panel.getBoundingClientRect().width,
            gutter: parseFloat(getComputedStyle(content).paddingLeft),
            contentWidth: content.clientWidth - parseFloat(getComputedStyle(content).paddingLeft) * 2,
            pathWidth: path?.getBoundingClientRect().width,
            agentsLast: !agentField || fields.every(field => field === agentField || field.getBoundingClientRect().bottom <= agentField.getBoundingClientRect().top),
            sourceLabel: fields.some(field => field.firstElementChild.textContent === 'Source'),
            labels: fields.map(field => ({ size: getComputedStyle(field.firstElementChild).fontSize, spacing: getComputedStyle(field.firstElementChild).letterSpacing })),
            agents: [...panel.querySelectorAll('.skill-agent-badge')].map(el => ({ height: el.getBoundingClientRect().height, size: getComputedStyle(el).fontSize })),
            overflow: content.scrollWidth > content.clientWidth,
          };
        }, panelId);
        assert.ok(Math.abs(geometry.width - width) < 2, JSON.stringify(geometry));
        assert.equal(geometry.gutter, width < 560 ? 16 : 24);
        assert.ok(geometry.labels.length >= 1 && geometry.labels.every(label => label.size === '12px' && ['normal', '0px'].includes(label.spacing)), JSON.stringify(geometry));
        assert.equal(geometry.sourceLabel, true);
        assert.equal(geometry.agentsLast, true);
        if (kind === 'skills') {
          assert.ok(geometry.pathWidth >= Math.min(geometry.contentWidth, 320) - 2, JSON.stringify(geometry));
          assert.ok(geometry.agents.length > 0 && geometry.agents.every(agent => Math.abs(agent.height - 20) < 1 && agent.size === '12px'), JSON.stringify(geometry));
        } else {
          assert.equal(geometry.agents.length, 0);
          assert.ok(geometry.pathWidth == null);
        }
        assert.equal(geometry.overflow, false);
        await assertDetailHeader(kind);
        await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `detail-${kind}-${width}.png`));
        if (kind === 'skills' && width === 360) {
          await $('[data-skill-item="global:cross-platform-testing-and-validation"][data-skill-open]').click();
          const expand = $('#skill-detail-panel button[aria-label="Expand description"]');
          await expand.waitForDisplayed();
          const descriptionId = await expand.getAttribute('aria-controls');
          const height = () => browser.execute(id => document.getElementById(id).getBoundingClientRect().height, descriptionId);
          const collapsedHeight = await height();
          assert.ok(collapsedHeight <= 45);
          await expand.click();
          await browser.waitUntil(async () => await height() > collapsedHeight + 20);
          await $('#skill-detail-panel button[aria-label="Collapse description"]').click();
          await browser.waitUntil(async () => Math.abs(await height() - collapsedHeight) < 1);
        }
      }
      }
    } finally {
      await browser.execute(saved => localStorage.setItem('skill-deck-layout', saved), saved);
      await browser.refresh();
    }
  });
});
