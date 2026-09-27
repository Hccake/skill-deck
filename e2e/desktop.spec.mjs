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
        await $('header').moveTo();
        await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `settings-${width}x${height}-${theme.toLowerCase()}.png`));
      }
    }
    await $('header nav a[href="/"]').click();
    await browser.waitUntil(async () => browser.execute(() => location.pathname === '/'));
  });

  it('preserves user panel widths and navigation focus in the real workspaces', async () => {
    const home = process.env.SKILL_DECK_E2E_HOME;
    const fixture = path.join(home, '.agents', 'skills', 'review');
    await mkdir(fixture, { recursive: true });
    await writeFile(path.join(fixture, 'SKILL.md'), '---\nname: review\ndescription: Desktop workspace validation.\n---\n# Workspace review\n\nVerify navigation, resizing, and keyboard focus.\n');
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
    await settle();
    assert.ok(Math.abs(await widthOf('skills') - 320) < 2);
    await browser.execute(() => { delete window.__sidebarBeforeResize; });
  });
});
