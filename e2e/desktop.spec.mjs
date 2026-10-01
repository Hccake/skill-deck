import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
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
    await browser.execute(({ width, height, actual }) => {
      window.__desktopViewport = { requested: { width, height }, actual };
    }, { width, height, actual });
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
  const state = await browser.execute(({ kind, part }) => {
    const preference = `${kind}${part === 'sidebar' ? 'Sidebar' : 'List'}Width`;
    const id = part === 'sidebar' ? `${kind}-context-navigation` : kind === 'skills' ? 'skills-list-panel' : 'library-skills-list-panel';
    return { width: document.getElementById(id).getBoundingClientRect().width,
      preferred: JSON.parse(localStorage.getItem('skill-deck-layout')).state[preference],
      active: document.activeElement.id, window: [innerWidth, innerHeight] };
  }, { kind, part });
  assert.ok(state.preferred !== null && Math.abs(state.width - state.preferred) < 2,
    `Keyboard resize must persist before another window change: ${JSON.stringify({ kind, part, ...state })}`);
}

async function assertDetailHeader(kind) {
  const geometry = await browser.execute(kind => {
    const panel = document.getElementById(kind === 'skills' ? 'skill-detail-panel' : 'library-skill-detail-panel');
    const title = panel.querySelector('h2');
    const row = title.parentElement.parentElement;
    const actions = row.lastElementChild;
    const rect = element => {
      const bounds = element.getBoundingClientRect();
      return { x: bounds.x, y: bounds.y, centerY: bounds.y + bounds.height / 2, right: bounds.right, width: bounds.width };
    };
    const text = document.createRange();
    text.selectNodeContents(title);
    return { row: rect(row), title: rect(title), textWidth: text.getBoundingClientRect().width, actions: rect(actions), gap: parseFloat(getComputedStyle(row).columnGap) };
  }, kind);
  assert.ok(geometry.textWidth + geometry.actions.width + geometry.gap <= geometry.row.width, `Short-title fixture must fit: ${JSON.stringify(geometry)}`);
  assert.ok(Math.abs(geometry.title.centerY - geometry.actions.centerY) < 2, `Title and actions should share a center line: ${JSON.stringify(geometry)}`);
  assert.ok(geometry.actions.right <= geometry.row.right + 1, JSON.stringify(geometry));
}

describe('Skill Deck desktop', () => {
  before(async () => {
    const expected = { win32: /Windows/, linux: /Linux/, darwin: /Macintosh/ }[process.platform];
    if (expected) {
      const userAgent = await browser.execute(() => navigator.userAgent);
      assert.match(userAgent, expected, 'The desktop driver must connect to the application on the target platform');
    }
  });

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

  it('expands Settings navigation at normal window widths and preserves compact navigation', async () => {
    await $('header nav a[href="/settings"]').click();
    await $('button=English').waitForDisplayed();
    await $('button=English').click();
    const records = [];
    for (const width of [800, 720, 766, 768, 1024, 1280, 800]) {
      await viewport(width, 600);
      const state = await browser.execute(() => {
        const sidebar = document.querySelector('aside');
        const main = document.querySelector('main');
        const entries = [...sidebar.querySelectorAll('button[aria-label]')];
        return {
          width: innerWidth,
          sidebar: sidebar.getBoundingClientRect().width,
          visibleLabels: entries.filter(button => button.lastElementChild.getBoundingClientRect().width > 0).length,
          entries: entries.length,
          overflow: main.scrollWidth > main.clientWidth || document.documentElement.scrollWidth > innerWidth,
          buttonsInside: entries.every(button => {
            const rect = button.getBoundingClientRect();
            return rect.left >= sidebar.getBoundingClientRect().left && rect.right <= sidebar.getBoundingClientRect().right;
          }),
        };
      });
      records.push(state);
      await writeFile(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'settings-navigation.json'), JSON.stringify(records, null, 2));
      if ([720, 800, 1280].includes(width)) {
        await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `settings-navigation-${width}.png`));
      }
      assert.equal(state.entries, 6);
      assert.equal(state.visibleLabels, width >= 768 ? 6 : 0, JSON.stringify(state));
      assert.ok(Math.abs(state.sidebar - (width >= 1024 ? 224 : width >= 768 ? 160 : 64)) < 1, JSON.stringify(state));
      assert.equal(state.overflow, false, JSON.stringify(state));
      assert.equal(state.buttonsInside, true, JSON.stringify(state));
    }
    await $('header nav a[href="/"]').click();
    await $('#skills-context-navigation').waitForDisplayed();
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
        const layout = await browser.execute(() => {
          const header = document.querySelector('header');
          const bounds = header.getBoundingClientRect();
          const navigation = header.querySelector('nav').getBoundingClientRect();
          return {
            overflow: document.documentElement.scrollWidth > innerWidth,
            headerHeight: bounds.height,
            navigationClearance: [navigation.top - bounds.top, bounds.bottom - parseFloat(getComputedStyle(header).borderBottomWidth) - navigation.bottom],
            navigation: [...header.querySelectorAll('nav a')].map((element) => {
              const rect = element.getBoundingClientRect();
              return rect.left >= 0 && rect.right <= innerWidth && rect.top >= 0 && rect.bottom <= innerHeight;
            }),
          };
        });
        assert.equal(layout.overflow, false);
        assert.equal(layout.headerHeight, 48);
        assert.ok(layout.navigationClearance.every(gap => gap >= 7), JSON.stringify(layout));
        assert.ok(layout.navigation.length >= 4 && layout.navigation.every(Boolean));
        const links = await browser.execute(() => [...document.querySelectorAll('header nav a')].map(link => ({
          height: link.getBoundingClientRect().height,
          labelVisible: link.querySelector('span').getBoundingClientRect().width > 0,
        })));
        assert.ok(links.every(link => Math.abs(link.height - 32) < 1), JSON.stringify(links));
        if (width === 720) assert.equal(links.filter(link => link.labelVisible).length, 1, JSON.stringify(links));
        if (width >= 1000) assert.ok(links.every(link => link.labelVisible), JSON.stringify(links));
        await $('header').moveTo();
        await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `settings-${width}x${height}-${theme.toLowerCase()}.png`));
      }
    }
    await $('header nav a[href="/"]').click();
    await browser.waitUntil(async () => browser.execute(() => location.pathname === '/'));
  });

  it('adapts navigation labels to the space left by surrounding controls', async () => {
    const labels = () => browser.execute(() => [...document.querySelectorAll('header nav a > span')].filter(el => el.getBoundingClientRect().width > 0).length);
    await $('header nav a[href="/settings"]').click();
    for (const locale of ['简体中文', 'English']) {
      await $(`button=${locale}`).click();
      await viewport(840, 600);
      assert.equal(await labels(), 4);
      for (const width of [720, 798]) {
        await viewport(width, 600);
        assert.equal(await labels(), 1, `${locale} at ${width}px should keep only the current label`);
        assert.equal(await $('header nav a[aria-current="page"] > span').isDisplayed(), true);
      }
    }
    await viewport(840, 600);
    assert.equal(await labels(), 4);
    const room = await browser.execute(() => {
      const nav = document.querySelector('header nav').getBoundingClientRect();
      const space = document.querySelector('header nav').parentElement.getBoundingClientRect();
      return [nav.left - space.left, space.right - nav.right];
    });
    assert.ok(room.every(width => width >= 24), JSON.stringify(room));
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
    await $('header nav a[href="/"]').click();
  });

  it('preserves user panel widths and navigation focus in the real workspaces', async () => {
    const home = process.env.SKILL_DECK_E2E_HOME;
    const fixture = path.join(home, '.agents', 'skills', 'review');
    await mkdir(fixture, { recursive: true });
    await writeFile(path.join(fixture, 'SKILL.md'), '---\nname: review\ndescription: Desktop workspace validation.\n---\n'
      + await readFile(new URL('./fixtures/detail-document.md', import.meta.url), 'utf8') + '\n\n'
      + await readFile(new URL('./fixtures/detail-resources.md', import.meta.url), 'utf8') + '\n\n'
      + Array.from({ length: 36 }, (_, index) => `## Section ${index + 1}\n\nVerify navigation, resizing, and keyboard focus while reading the current document. Keep the selected Skill and its maintenance commands available.\n`).join('\n'));
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

  it('keeps technical content within each detail pane and exposes horizontal scrolling', async () => {
    await $('header nav a[href="/"]').waitForDisplayed();
    const saved = await browser.execute(() => localStorage.getItem('skill-deck-layout'));
    try {
      await viewport(1200, 640);
      for (const [kind, route, panelId] of [['skills', '/', 'skill-detail-panel'], ['library', '/libraries', 'library-skill-detail-panel']]) {
        for (const width of [360, 640]) {
          await browser.execute(({ saved, kind, width }) => {
            const layout = JSON.parse(saved);
            Object.assign(layout.state, { skillsSidebarWidth: 208, librarySidebarWidth: 208 });
            layout.state[kind === 'skills' ? 'skillsListWidth' : 'libraryListWidth'] = 991 - width;
            localStorage.setItem('skill-deck-layout', JSON.stringify(layout));
          }, { saved, kind, width });
          await browser.refresh();
          await $(`header nav a[href="${route}"]`).waitForDisplayed();
          await $(`header nav a[href="${route}"]`).click();
          await $(`#${kind}-context-navigation`).waitForDisplayed();
          if (kind === 'skills') await $('#skills-context-navigation aside > div:first-child button').click();
          else await $('#library-context-navigation').$('button*=Workspace review').click();
          await $('[data-skill-open]').waitForDisplayed();
          await browser.execute(key => [...document.querySelectorAll('[data-skill-open]')]
            .find(el => el.closest('[data-skill-item]').dataset.skillItem === key).click(), kind === 'skills' ? 'global:review' : 'review');
          await $(`#${panelId} .skill-code-block`).waitForDisplayed();
          await settle();
          await browser.execute(panelId => document.getElementById(panelId).querySelector('.skill-code-block[data-multiline="true"] .skill-code-copy').focus({ preventScroll: true }), panelId);
          await settle();
          const result = await browser.execute(panelId => {
            const root = document.getElementById(panelId), prose = root.querySelector('.skill-prose');
            const codes = [...prose.querySelectorAll('pre')], tables = [...prose.querySelectorAll('.skill-table-scroll')];
            const copy = prose.querySelector('.skill-code-copy'), pre = codes[0];
            pre.scrollLeft = 100;
            tables[1].scrollLeft = 100;
            return { width: root.getBoundingClientRect().width, overflow: prose.scrollWidth > prose.clientWidth,
              codeScroll: pre.scrollLeft, tableScroll: tables[1].scrollLeft, codeTab: pre.tabIndex, tableTab: tables[1].tabIndex,
              tableOverflow: tables[1].scrollWidth > tables[1].clientWidth + 1,
              shortTableTab: tables[0].tabIndex, tableCount: tables.length,
              copyCount: prose.querySelectorAll('.skill-code-copy').length,
              copySeparated: copy.getBoundingClientRect().left >= pre.getBoundingClientRect().right,
              multilineLayouts: codes.slice(1).map(code => {
                const block = code.parentElement;
                const topSpace = code.getBoundingClientRect().top - block.getBoundingClientRect().top + parseFloat(getComputedStyle(code).paddingTop);
                return { widthDelta: block.clientWidth - code.clientWidth, topSpace };
              }),
              copyFocusOpacity: getComputedStyle(document.activeElement).opacity,
              copyHasFocus: document.activeElement === codes[1].parentElement.querySelector('.skill-code-copy'),
              tableHeaderBackground: getComputedStyle(prose.querySelector('thead tr')).backgroundColor,
              tableRowBackgrounds: [...tables[0].querySelectorAll('tbody tr')].map(row => getComputedStyle(row).backgroundColor),
              tableBorders: [...tables[0].querySelectorAll('th, td')].map(cell => {
                const style = getComputedStyle(cell);
                return [style.borderTopWidth, style.borderRightWidth, style.borderBottomWidth, style.borderLeftWidth];
              }),
              codeSize: getComputedStyle(pre.firstElementChild).fontSize,
              quoteStyle: getComputedStyle(prose.querySelector('blockquote')).fontStyle };
          }, panelId);
          assert.ok(Math.abs(result.width - width) < 2 && !result.overflow, JSON.stringify(result));
          assert.ok(result.codeScroll > 0 && result.codeTab === 0, JSON.stringify(result));
          assert.equal(result.tableTab, result.tableOverflow ? 0 : -1, JSON.stringify(result));
          assert.equal(result.tableScroll > 0, result.tableOverflow, JSON.stringify(result));
          if (width === 360) assert.equal(result.tableOverflow, true, JSON.stringify(result));
          assert.ok(result.shortTableTab === -1 && result.tableCount === 2 && result.copyCount === 3 && result.copySeparated, JSON.stringify(result));
          assert.ok(result.multilineLayouts.every(layout => Math.abs(layout.widthDelta) < 2 && layout.topSpace <= 12), JSON.stringify(result));
          assert.ok(result.copyHasFocus && result.copyFocusOpacity === '1', JSON.stringify(result));
          assert.equal(result.tableHeaderBackground, result.tableRowBackgrounds[0], JSON.stringify(result));
          assert.ok(result.tableRowBackgrounds.length >= 2 && result.tableRowBackgrounds[0] !== result.tableRowBackgrounds[1], JSON.stringify(result));
          assert.ok(result.tableBorders.every(borders => borders.every(width => parseFloat(width) > 0)), JSON.stringify(result));
          assert.equal(result.codeSize, '13px');
          assert.equal(result.quoteStyle, 'normal');
        }
      }
    } finally {
      await browser.execute(saved => localStorage.setItem('skill-deck-layout', saved), saved);
      await browser.refresh();
    }
  });

  it('navigates document sections without leaving either detail and shows unsupported resources inline', async () => {
    await $('header nav a[href="/"]').waitForDisplayed();
    for (const [kind, route, panelId] of [['skills', '/', 'skill-detail-panel'], ['library', '/libraries', 'library-skill-detail-panel']]) {
      await viewport(1200, 640);
      await $(`header nav a[href="${route}"]`).click();
      await $(`#${kind}-context-navigation`).waitForDisplayed();
      if (kind === 'skills') await $('#skills-context-navigation aside > div:first-child button').click();
      else await $('#library-context-navigation').$('button*=Workspace review').click();
      await $('[data-skill-open]').waitForDisplayed();
      await browser.execute(key => [...document.querySelectorAll('[data-skill-open]')]
        .find(el => el.closest('[data-skill-item]').dataset.skillItem === key).click(), kind === 'skills' ? 'global:review' : 'review');
      await $(`#${panelId} .skill-document-link`).waitForExist();
      await viewport(720, 520);
      const locationBefore = await browser.getUrl();
      // embedded 驱动会在 click 后强制回焦；此处按先聚焦再激活的顺序验证应用行为。
      await browser.execute(panelId => {
        const link = document.getElementById(panelId).querySelector('a[href="#重复章节-1"]');
        link.focus({ preventScroll: true });
        link.click();
      }, panelId);
      const chapter = await browser.execute(panelId => {
        const root = document.getElementById(panelId), viewport = root.querySelector('[data-radix-scroll-area-viewport]');
        return { anchor: document.activeElement.dataset.documentAnchor,
          offset: document.activeElement.getBoundingClientRect().top - viewport.getBoundingClientRect().top,
          firstMargin: getComputedStyle(root.querySelector('.skill-markdown').firstElementChild).marginTop,
          overflow: root.querySelector('.skill-prose').scrollWidth > root.querySelector('.skill-prose').clientWidth,
          imageFallbacks: root.querySelectorAll('.skill-image-fallback').length,
          unsafeControls: root.querySelectorAll('a button, a a').length,
          images: root.querySelectorAll('.skill-markdown img').length };
      }, panelId);
      assert.equal(chapter.anchor, '重复章节-1');
      assert.equal(chapter.firstMargin, '0px');
      assert.ok(Math.abs(chapter.offset - 8) < 2, JSON.stringify(chapter));
      assert.ok(!chapter.overflow && chapter.imageFallbacks === 2 && chapter.images === 0 && chapter.unsafeControls === 0, JSON.stringify(chapter));
      assert.equal(await browser.getUrl(), locationBefore);
      await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `detail-resources-${kind}-720.png`));
      await browser.execute(panelId => {
        const link = document.getElementById(panelId).querySelector('a[href="#"]');
        link.focus({ preventScroll: true });
        link.click();
      }, panelId);
      assert.equal(await browser.execute(() => document.activeElement.classList.contains('skill-markdown')), true);
      assert.equal(await browser.getUrl(), locationBefore);
    }
  });

  it('keeps detail commands visible during long reading and resets only for another object', async () => {
    await $('header nav a[href="/"]').waitForDisplayed();
    await viewport(1200, 640);
    for (const [kind, route, panelId, listId] of [
      ['skills', '/', 'skill-detail-panel', 'skills-list-panel'],
      ['library', '/libraries', 'library-skill-detail-panel', 'library-skills-list-panel'],
    ]) {
      await $(`header nav a[href="${route}"]`).click();
      await $(`#${kind}-context-navigation`).waitForDisplayed();
      if (kind === 'skills') await $('#skills-context-navigation aside > div:first-child button').click();
      else await $('#library-context-navigation').$('button*=Workspace review').click();
      const select = async name => {
        await $(`#${listId} [data-skill-open]`).waitForDisplayed();
        await browser.execute(({ listId, key }) => {
          const item = [...document.getElementById(listId).querySelectorAll('[data-skill-open]')]
            .find(el => el.closest('[data-skill-item]').dataset.skillItem === key);
          item.click();
        }, { listId, key: kind === 'skills' ? `global:${name}` : name });
        await $(`#${panelId} .skill-prose`).waitForDisplayed();
        await settle();
      };
      await select('review');
      await viewport(720, 520);
      const initial = await browser.execute(panelId => {
        const root = document.getElementById(panelId);
        const header = root.querySelector('.skill-detail-header').getBoundingClientRect();
        const viewport = root.querySelector('[data-radix-scroll-area-viewport]');
        viewport.scrollTop = 1400;
        return { y: header.y, height: header.height };
      }, panelId);
      await settle();
      const reading = await browser.execute(panelId => {
        const root = document.getElementById(panelId);
        const header = root.querySelector('.skill-detail-header').getBoundingClientRect();
        return { y: header.y, height: header.height, scroll: root.querySelector('[data-radix-scroll-area-viewport]').scrollTop };
      }, panelId);
      assert.ok(Math.abs(reading.y - initial.y) < 1 && reading.height <= 77 && reading.scroll > 1000, JSON.stringify(reading));
      await select('review');
      assert.ok(await browser.execute(panelId => document.getElementById(panelId).querySelector('[data-radix-scroll-area-viewport]').scrollTop > 1000, panelId));
      if (kind === 'skills') {
        const command = `#${panelId} button[aria-label="Manage linked Agents"]`;
        await browser.execute(selector => document.querySelector(selector).focus(), command);
        await $(command).click();
        await $('[role="dialog"]').waitForDisplayed();
        await $('[role="dialog"]').$('button=Cancel').click();
        await $('[role="dialog"]').waitForExist({ reverse: true });
        await browser.waitUntil(async () => browser.execute(selector => document.activeElement === document.querySelector(selector), command));
        assert.ok(await browser.execute(panelId => document.getElementById(panelId).querySelector('[data-radix-scroll-area-viewport]').scrollTop > 1000, panelId));
      }
      await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `detail-reading-${kind}-720.png`));
      await select('code-review');
      assert.equal(await browser.execute(panelId => document.getElementById(panelId).querySelector('[data-radix-scroll-area-viewport]').scrollTop, panelId), 0);
      await viewport(1200, 640);
    }
  });
  it('browses Discover with separate keyboard focus and selection', async () => {
    // embedded 1.4 合成按键不执行按钮默认激活；Enter/Space 用目标平台受信输入另验。
    // 真实在线排行榜验证 WebView 交互；请求乱序和失败恢复由组件测试使用可控响应覆盖。
    await $('header nav a[href="/discover"]').click();
    await $('#discover-list-fixed [data-skill-open]').waitForDisplayed({ timeout: 30000 });
    const rows = await browser.execute(() => [...document.querySelectorAll('#discover-list-fixed [data-skill-open]')].slice(0, 2).map(row => ({
      key: row.dataset.skillItem, name: row.querySelector('.font-heading').textContent,
    })));
    assert.equal(rows.length, 2);
    await browser.execute(() => document.querySelector('#discover-list-fixed [data-skill-open]').focus());
    await browser.keys('ArrowDown');
    assert.equal(await browser.execute(() => document.activeElement.dataset.skillItem), rows[1].key);
    assert.equal(await $('#discover-detail-fixed h2').isExisting(), false);
    await browser.execute(() => document.activeElement.click());
    await $('#discover-detail-fixed h2').waitForDisplayed();
    assert.equal(await $('#discover-detail-fixed h2').getText(), rows[1].name);
    await browser.keys('ArrowUp');
    assert.equal(await $('#discover-detail-fixed h2').getText(), rows[1].name);
    assert.equal(await browser.execute(() => document.activeElement.dataset.skillItem), rows[0].key);
    await browser.execute(() => document.activeElement.click());
    await browser.waitUntil(async () => await $('#discover-detail-fixed h2').getText() === rows[0].name);
    await browser.execute(() => document.querySelector('#discover-detail-fixed button[aria-label]').focus());
    await browser.execute(() => document.activeElement.click());
    assert.equal(await browser.execute(() => document.activeElement.dataset.skillItem), rows[0].key);
    await browser.execute(() => document.activeElement.click());
    await browser.keys(['Control', 'f']);
    assert.equal(await browser.execute(() => document.activeElement.matches('input[data-skill-search]')), true);
    await $('input[data-skill-search]').setValue('discover-keyboard-review-no-results');
    await browser.waitUntil(async () => !(await $('[role="tab"]').isExisting()));
    await browser.keys('Escape');
    assert.equal(await $('input[data-skill-search]').getValue(), '');
    assert.equal(await $('#discover-detail-fixed h2').getText(), rows[0].name);
    await $('#discover-list-fixed [data-skill-open]').waitForDisplayed();
    await browser.keys('Escape');
    assert.equal(await browser.execute(() => document.activeElement.dataset.skillItem), rows[0].key);
    assert.equal(await $('#discover-detail-fixed h2').isExisting(), false);
    await $('header nav a[href="/"]').click();
    await $('#skills-context-navigation').waitForDisplayed();
  });

  it('remembers Discover panel width without overwriting automatic or other page preferences', async () => {
    await $('header nav a[href="/discover"]').click();
    await $('#discover-list-fixed').waitForDisplayed();
    const dimensions = () => browser.execute(() => ({
      list: document.getElementById('discover-list-fixed').getBoundingClientRect().width,
      detail: document.getElementById('discover-detail-fixed').getBoundingClientRect().width,
      saved: JSON.parse(localStorage.getItem('skill-deck-layout'))?.state ?? {},
    }));
    await settle();
    const initial = await dimensions();
    const records = [];
    for (const [width, expected] of [[720, 240], [800, 240], [1280, 320]]) {
      await viewport(width, 600);
      const current = await dimensions();
      assert.ok(Math.abs(current.list - expected) < 2, JSON.stringify(current));
      assert.ok(current.detail >= 359, JSON.stringify(current));
      assert.equal(current.saved.discoverListWidth ?? null, null);
      records.push({ width, ...current });
    }
    await browser.execute(() => document.getElementById('discover-detail-resize').focus());
    await browser.keys('ArrowRight');
    await browser.keys('ArrowRight');
    await settle();
    const manual = await dimensions();
    assert.ok(manual.saved.discoverListWidth > 400 && Math.abs(manual.list - manual.saved.discoverListWidth) < 2, JSON.stringify(manual));
    await viewport(720, 520);
    const narrow = await dimensions();
    assert.equal(narrow.saved.discoverListWidth, manual.saved.discoverListWidth);
    assert.ok(narrow.list >= 239 && narrow.detail >= 359, JSON.stringify(narrow));
    await viewport(1280, 800);
    await browser.refresh();
    await $('header nav a[href="/discover"]').waitForDisplayed();
    await $('header nav a[href="/discover"]').click();
    await $('#discover-list-fixed').waitForDisplayed();
    await settle();
    const restored = await dimensions();
    assert.ok(Math.abs(restored.list - manual.saved.discoverListWidth) < 2, JSON.stringify(restored));
    for (const key of ['skillsListWidth', 'libraryListWidth', 'skillsSidebarWidth', 'librarySidebarWidth']) {
      assert.equal(restored.saved[key] ?? null, initial.saved[key] ?? (key.endsWith('SidebarWidth') ? 208 : null));
    }
    await writeFile(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'discover-widths.json'), JSON.stringify({ records, manual, narrow, restored }, null, 2));
    await $('header nav a[href="/"]').click();
    await $('#skills-context-navigation').waitForDisplayed();
  });

  it('keeps Discover commands fixed and adapts reading and decision information to the detail width', async () => {
    await viewport(1280, 800);
    await $('header nav a[href="/discover"]').click();
    await $('#discover-list-fixed [data-skill-open]').waitForDisplayed({ timeout: 30000 });
    await $('#discover-list-fixed [data-skill-open]').click();
    await browser.waitUntil(() => browser.execute(() => !document.querySelector('#discover-detail-fixed [data-testid="discover-detail-skeleton"]')));
    const records = [];
    for (const wide of [false, true]) {
      await browser.execute(() => {
        const separator = document.getElementById('discover-detail-resize');
        separator.focus();
        separator.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }));
      });
      if (wide) for (let index = 0; index < 7; index++) await browser.keys('ArrowLeft');
      await settle();
      const state = await browser.execute(() => {
        const root = document.getElementById('discover-detail-fixed');
        const header = root.querySelector('.discover-detail-header');
        const reading = root.querySelector('.discover-reading');
        const decision = root.querySelector('.discover-decision-info');
        const viewport = root.querySelector('[data-radix-scroll-area-viewport]');
        viewport.scrollTop = 0;
        const before = header.getBoundingClientRect().top;
        viewport.scrollTop = 180;
        return { width: root.getBoundingClientRect().width, before, after: header.getBoundingClientRect().top,
          identityCenter: (() => { const r = header.querySelector('.discover-header-identity')?.getBoundingClientRect(); return r ? r.top + r.height / 2 : null; })(),
          actionsCenter: (() => { const r = header.querySelector('button').parentElement.getBoundingClientRect(); return r.top + r.height / 2; })(),
          headerHeight: header.getBoundingClientRect().height, headerHasSource: Boolean(header.querySelector('.discover-source-name')),
          source: header.querySelector('.discover-source-name')?.textContent,
          sourceInside: header.querySelector('.discover-source-name')?.getBoundingClientRect().right <= header.getBoundingClientRect().right + 1,
          overflow: root.scrollWidth > root.clientWidth || reading.scrollWidth > reading.clientWidth,
          decisionRight: decision ? decision.getBoundingClientRect().left > reading.getBoundingClientRect().left : null,
          decisionBefore: decision ? decision.getBoundingClientRect().top < reading.getBoundingClientRect().top : null,
          buttonsInside: [...header.querySelectorAll('button')].every(button => button.getBoundingClientRect().right <= root.getBoundingClientRect().right + 1),
        };
      });
      assert.ok(wide ? state.width > 760 : Math.abs(state.width - 360) < 2, JSON.stringify(state));
      assert.equal(state.before, state.after);
      assert.equal(state.headerHasSource, true);
      assert.equal(state.sourceInside, true);
      assert.equal(state.overflow, false);
      assert.equal(state.buttonsInside, true);
      if (wide) assert.ok(state.identityCenter !== null && Math.abs(state.identityCenter - state.actionsCenter) < 2, JSON.stringify(state));
      if (state.decisionRight !== null) assert.equal(wide ? state.decisionRight : state.decisionBefore, wide, JSON.stringify(state));
      records.push(state);
    }
    await writeFile(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'discover-reading.json'), JSON.stringify(records, null, 2));
    await $('header nav a[href="/"]').click();
    await $('#skills-context-navigation').waitForDisplayed();
  });

});
