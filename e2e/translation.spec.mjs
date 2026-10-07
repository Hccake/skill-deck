import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { browser, $ } from '@wdio/globals';

async function invoke(command, args) {
  const result = await browser.executeAsync((command, args, done) => {
    window.__TAURI_INTERNALS__.invoke(command, args).then(value => done({ value }), error => done({ error }));
  }, command, args);
  assert.equal(result.error, undefined, JSON.stringify(result.error));
  return result.value;
}

async function titleLayout(selector) {
  return browser.execute(selector => {
    const root = document.querySelector(selector);
    const rect = element => {
      const box = element.getBoundingClientRect();
      return { x: box.x, y: box.y, width: box.width, height: box.height, center: box.y + box.height / 2 };
    };
    return { title: rect(root.querySelector('h2')), buttons: [...root.querySelectorAll('button')].map(rect) };
  }, selector);
}

function assertStableTitle(before, after) {
  assert.equal(after.title.y, before.title.y);
  assert.ok(Math.abs(after.buttons[0].center - after.title.center) < 1, '翻译按钮与原名首行对齐');
  assert.equal(after.buttons[0].y, before.buttons[0].y);
  before.buttons.forEach((button, index) => {
    assert.equal(after.buttons[index].x, button.x);
    if (Math.abs(button.center - before.title.center) < 1) assert.equal(after.buttons[index].y, button.y);
  });
}

async function assertBilingualStyle(selector) {
  const styles = await browser.execute(selector => {
    return [...document.querySelectorAll(`${selector} p > .skill-translation:not([hidden])`)].map(node => {
      const translated = getComputedStyle(node);
      const original = getComputedStyle(node.parentElement);
      return { translated: translated.color, original: original.color, font: translated.fontSize, originalFont: original.fontSize };
    });
  }, selector);
  assert.ok(styles.length > 0);
  styles.forEach(style => {
    assert.notEqual(style.translated, style.original, '原文与译文应通过文字颜色区分');
    assert.equal(style.font, style.originalFont, '正文译文应保持同等可读字号');
  });
}

describe('Skill detail translation', () => {
  let fixture;
  it('translates title, description and document in the native WebView and keeps the toggle stable', async () => {
    const folder = path.join(process.env.SKILL_DECK_E2E_HOME, '.agents', 'skills', 'translation-sample');
    fixture = folder;
    await mkdir(folder, { recursive: true });
    await writeFile(path.join(folder, 'SKILL.md'), '---\nname: translation-sample\ndescription: Read the skill instructions before making changes.\n---\n# Installation\n\nRead the guide before making changes.\n\nRun `npm install` to install dependencies.\n');
    await $('header nav a[href="/settings"]').waitForDisplayed();
    const settings = await invoke('get_translation_settings');
    await invoke('save_translation_settings', { draft: { expectedRevision: settings.revision, service: { kind: 'microsoft' }, profiles: [], autoTranslate: false, noticeAccepted: true } });
    await browser.refresh();
    await $('[data-testid="skill-card-title"]').waitForDisplayed({ timeout: 30000 });
    await $('[data-testid="skill-card-title"]').click();
    const toggle = $('button[aria-label="Translate"][aria-pressed]');
    await toggle.waitForDisplayed();
    const label = await toggle.getAttribute('aria-label');
    assert.equal(await toggle.getText(), '');
    assert.ok(await browser.execute(() => Boolean(document.querySelector('.skill-detail-heading button[aria-pressed]'))));
    assert.equal((await browser.$$('.skill-detail-actions button[aria-pressed]')).length, 0);
    assert.equal((await browser.$$('button[aria-label="Translation options"]')).length, 0);
    const titleBefore = await titleLayout('.skill-detail-header');
    await toggle.click();
    await browser.waitUntil(async () => (await browser.$$('[data-translation-output]')).length >= 3, { timeout: 60000 });
    assertStableTitle(titleBefore, await titleLayout('.skill-detail-header'));
    assert.equal(await toggle.getAttribute('aria-label'), label);
    assert.equal(await toggle.getAttribute('aria-pressed'), 'true');
    assert.ok(await $('.skill-detail-heading [data-translation-output]').isDisplayed());
    assert.ok(await $('.skill-description [data-translation-output]').isDisplayed());
    assert.ok(await $('.skill-markdown [data-translation-output]').isDisplayed());
    await assertBilingualStyle('.skill-markdown');
    assert.equal(await $('.skill-markdown code').getText(), 'npm install');
    const runtime = await browser.execute(() => ({ userAgent: navigator.userAgent, platform: navigator.platform, translations: document.querySelectorAll('[data-translation-output]').length }));
    const platformPattern = { win32: /Windows/, linux: /Linux/, darwin: /Macintosh/ }[process.platform];
    assert.match(runtime.userAgent, platformPattern);
    await browser.execute(() => document.activeElement?.blur());
    await $('header').moveTo();
    for (const dark of [false, true]) {
      await browser.execute(dark => document.documentElement.classList.toggle('dark', dark), dark);
      await browser.setWindowRect(null, null, 800, 650);
      await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, `translation-${dark ? 'dark' : 'light'}.png`));
    }
    await writeFile(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'translation-runtime.json'), JSON.stringify(runtime, null, 2));
    await toggle.click();
    await browser.waitUntil(async () => (await browser.$$('[data-translation-output]')).length === 0);
    assert.equal(await toggle.getAttribute('aria-pressed'), 'false');
    assert.ok((await $('.skill-markdown').getText()).includes('Read the guide'));
  });
  it('uses the Google adapter and returns from translation settings to the selected detail', async () => {
    const result = await invoke('test_translation_service', { service: { kind: 'google' }, profile: null });
    assert.match(result, /[\u3400-\u9fff]/);
    await $('header nav a[href="/settings"]').click();
    await $('button[aria-label="Translation settings"]').waitForDisplayed();
    await $('button[aria-label="Translation settings"]').click();
    await $('#translation-service').waitForDisplayed();
    assert.match(await $('#translation-service').getText(), /微软翻译|Microsoft/);
    await $('header nav a[href="/"]').click();
    await $('.skill-detail-heading').waitForDisplayed();
    assert.ok((await $('.skill-detail-heading').getText()).includes('translation-sample'));
  });

  it('uses the same bilingual reading controls in a skill library', async () => {
    const environment = { kind: 'native' };
    const catalog = await invoke('create_skill_library', { environment, name: 'Translation library' });
    const libraryId = catalog.libraries.find(library => library.name === 'Translation library').id;
    const source = await invoke('discover_skill_source', { environment, source: fixture, operationId: 'translation-library', selectionIntent: { wildcardRequested: true, explicitSkillNames: [] } });
    const payloads = await invoke('acquire_selected_payloads', { request: { discoverySession: source.discoverySession, skillPaths: source.skills.map(skill => skill.relativePath) } });
    const request = { environment, libraryId, discoverySession: source.discoverySession, skills: source.skills.map(skill => ({ skillName: skill.installDirName, payload: payloads.find(payload => payload.skillPath === skill.relativePath) })) };
    const preview = await invoke('preview_add_library_skills', { request });
    await invoke('add_skills_to_library', { request: { request, expectedToken: preview.token, membership: preview.membership, acknowledgeRedirect: false } });
    await $('header nav a[href="/libraries"]').click();
    await $('[data-testid="library-skill-title"] button').waitForDisplayed();
    await $('[data-testid="library-skill-title"] button').click();
    const toggle = $('button[aria-label="Translate"][aria-pressed]');
    await toggle.waitForDisplayed();
    const titleBefore = await titleLayout('.skill-detail-header');
    await toggle.click();
    await browser.waitUntil(async () => (await browser.$$('[data-translation-output]')).length >= 3, { timeout: 60000 });
    assertStableTitle(titleBefore, await titleLayout('.skill-detail-header'));
    assert.ok(await $('.skill-description [data-translation-output]').isDisplayed());
    await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'translation-library.png'));
    await $('header nav a[href="/settings"]').click();
    await $('button[aria-label="Translation settings"]').waitForDisplayed();
    await $('button[aria-label="Translation settings"]').click();
    await $('#translation-service').waitForDisplayed();
    await $('header nav a[href="/libraries"]').click();
    await $('.skill-detail-heading').waitForDisplayed();
    assert.ok((await $('.skill-detail-heading').getText()).includes('translation-sample'));
  });

  it('translates sanitized Discover HTML in the real WebView', async () => {
    try {
      await $('header nav a[href="/discover"]').click();
      await $('#discover-list-fixed [data-skill-open]').waitForDisplayed();
      await $('#discover-list-fixed [data-skill-open]').click();
      await $('.discover-rich-text').waitForDisplayed();
      assert.equal((await browser.$$('.discover-header-actions button[aria-pressed]')).length, 0);
      assert.ok(await $('.discover-header-actions').$('button=Install').isDisplayed());
      const titleBefore = await titleLayout('.discover-detail-header');
      await $('.discover-heading-row button[aria-pressed]').click();
      await browser.waitUntil(async () => (await browser.$$('.discover-rich-text [data-translation-output]:not([hidden])')).length >= 3, { timeout: 60000 });
      assertStableTitle(titleBefore, await titleLayout('.discover-detail-header'));
      await assertBilingualStyle('.discover-rich-text');
      assert.equal(await $('.discover-rich-text code').getText(), 'npm install');
      await browser.waitUntil(async () => (await browser.$$('.discover-rich-text blockquote [data-translation-output]:not([hidden])')).length === 2);
      assert.equal(await $('.discover-rich-text pre').getText(), 'Read diagram\nA -> B');
      assert.equal((await browser.$$('.discover-rich-text pre [data-translation-output]')).length, 0);
      const undecorated = await browser.execute(() => [...document.querySelectorAll('[data-translation-output]:not([hidden])')].every(node => {
        const style = getComputedStyle(node);
        return ['transparent', 'rgba(0, 0, 0, 0)'].includes(style.backgroundColor)
          && style.paddingLeft === '0px' && style.paddingRight === '0px'
          && style.borderTopWidth === '0px' && style.borderLeftWidth === '0px'
          && style.boxShadow === 'none';
      }));
      assert.equal(undecorated, true, '译文自身不能增加文字框');
      await browser.setWindowRect(null, null, 720, 650);
      await browser.execute(() => document.getElementById('discover-detail-resize').focus());
      for (let index = 0; index < 4; index++) await browser.keys('ArrowRight');
      const narrow = await browser.execute(() => {
        const heading = document.querySelector('.discover-header-identity h2');
        const panel = document.getElementById('discover-detail-fixed');
        return { width: panel.getBoundingClientRect().width, titleWidth: heading.clientWidth, titleScroll: heading.scrollWidth, headerHeight: document.querySelector('.discover-detail-header').getBoundingClientRect().height };
      });
      assert.ok(narrow.titleScroll <= narrow.titleWidth + 1, JSON.stringify(narrow));
      assert.ok(narrow.titleWidth >= 200 && narrow.headerHeight <= 180, JSON.stringify(narrow));
      await browser.waitUntil(async () => (await browser.$$('.discover-rich-text [data-translation-output]:not([hidden])')).length >= 3);
      await writeFile(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'translation-narrow.json'), JSON.stringify(narrow));
      await browser.execute(() => document.activeElement?.blur());
      await $('header').moveTo();
      await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'translation-discover.png'));
      await browser.execute(() => {
        const viewport = document.querySelector('.discover-detail [data-radix-scroll-area-viewport]');
        viewport.scrollTop = viewport.scrollHeight;
      });
      await browser.executeAsync(done => requestAnimationFrame(() => requestAnimationFrame(done)));
      await browser.saveScreenshot(path.join(process.env.SKILL_DECK_E2E_OUTPUT, 'translation-quote-and-pre.png'));
      await $('header nav a[href="/settings"]').click();
      await $('button[aria-label="Translation settings"]').waitForDisplayed();
      await $('button[aria-label="Translation settings"]').click();
      await $('button=Return to skill details').waitForDisplayed();
      await $('button=Return to skill details').click();
      await $('.discover-rich-text').waitForDisplayed();
      assert.ok((await $('.discover-header-identity h2').getText()).includes('Translation guide'));
    } finally {
      await $('header nav a[href="/"]').click();
    }
  });

});
