import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createWriteStream } from 'node:fs';
import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { remote } from 'webdriverio';

export async function verifyWindowState(binary, run, port) {
  const statePath = path.join(run.env.SKILL_DECK_E2E_HOME, '.window-state.json');
  const records = [];
  let child;
  let browser;
  let log;

  async function launch() {
    log = createWriteStream(path.join(run.directory, 'window-state.log'), { flags: 'a' });
    child = spawn(binary, [], {
      env: { ...process.env, ...run.env, TAURI_WEBDRIVER_PORT: String(port) },
      stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true,
    });
    child.stdout.pipe(log, { end: false });
    child.stderr.pipe(log, { end: false });
    let launchError;
    child.once('error', error => { launchError = error; });
    const deadline = Date.now() + 20000;
    for (;;) {
      if (launchError) throw launchError;
      if (child.exitCode !== null) throw new Error(`App exited during startup: ${child.exitCode}`);
      try {
        const status = await (await fetch(`http://127.0.0.1:${port}/status`, { signal: AbortSignal.timeout(1000) })).json();
        if (status.value?.ready) break;
      } catch { /* Wait for the embedded driver to start. */ }
      if (Date.now() >= deadline) throw new Error('Window-state driver did not start');
      await delay(100);
    }
    browser = await remote({ hostname: '127.0.0.1', port, logLevel: 'silent', connectionRetryCount: 0, capabilities: { browserName: 'tauri' } });
    await browser.$('header nav').waitForDisplayed({ timeout: 15000 });
  }

  async function invoke(command, args = {}) {
    const result = await browser.executeAsync((command, args, done) => {
      window.__TAURI_INTERNALS__.invoke(command, args).then(value => done({ value }), error => done({ error }));
    }, command, args);
    assert.equal(result.error, undefined, `${command}: ${JSON.stringify(result.error)}`);
    return result.value;
  }

  async function geometry(name) {
    const value = await invoke('plugin:window|inner_size', { label: 'main' });
    const result = {
      name, size: value,
      position: await invoke('plugin:window|outer_position', { label: 'main' }),
      outer: await invoke('plugin:window|outer_size', { label: 'main' }),
      maximized: await invoke('plugin:window|is_maximized', { label: 'main' }),
      minimized: await invoke('plugin:window|is_minimized', { label: 'main' }),
      visible: await invoke('plugin:window|is_visible', { label: 'main' }),
      monitor: await invoke('plugin:window|current_monitor', { label: 'main' }),
    };
    records.push(result);
    await writeFile(path.join(run.directory, 'window-state.json'), JSON.stringify(records, null, 2));
    return result;
  }

  async function stop() {
    // Leave through the application's existing lifecycle, allowing the plugin to save on exit.
    // Process exit can close the driver before it acknowledges this command; verify the exit below.
    await browser.execute(() => { void window.__TAURI_INTERNALS__.invoke('execute_lifecycle_action', { action: 'quitApplication' }); }).catch(() => {});
    browser = null;
    const deadline = Date.now() + 10000;
    while (child.exitCode === null) {
      if (Date.now() >= deadline) throw new Error('Application did not finish its guarded exit');
      await delay(50);
    }
    assert.equal(child.exitCode, 0);
    child.stdout.unpipe(log);
    child.stderr.unpipe(log);
    child.stdout.destroy();
    child.stderr.destroy();
    log.end();
    child = null;
  }

  function sameBounds(actual, expected) {
    for (const key of ['width', 'height']) assert.ok(Math.abs(actual.size[key] - expected.size[key]) <= 2, `${key}: ${JSON.stringify({ actual, expected })}`);
    for (const key of ['x', 'y']) assert.ok(Math.abs(actual.position[key] - expected.position[key]) <= 2, `${key}: ${JSON.stringify({ actual, expected })}`);
  }

  try {
    await launch();
    const initial = await geometry('initial');
    assert.equal(initial.visible, true);
    assert.equal(initial.maximized, false);
    const scale = initial.monitor.scaleFactor;
    const work = initial.monitor.workArea;
    const width = Math.max(720, Math.min(900, Math.floor(work.size.width / scale) - 60));
    const height = Math.max(520, Math.min(640, Math.floor(work.size.height / scale) - 80));
    await browser.setWindowRect(Math.round(work.position.x / scale + 30), Math.round(work.position.y / scale + 30), width, height);
    const normal = await geometry('chosen-normal');
    await invoke('open_install_wizard', {
      entryPoint: 'skills-panel',
      context: { environment: { kind: 'native' }, scope: { scope: 'global' } },
      projectPath: null, prefillSource: null, prefillSkillName: null,
    });
    const mainHandle = await browser.getWindowHandle();
    await browser.waitUntil(async () => (await browser.getWindowHandles()).length === 2);
    const wizardHandle = (await browser.getWindowHandles()).find(handle => handle !== mainHandle);
    await browser.switchToWindow(wizardHandle);
    const cancel = await browser.$('//button[normalize-space(.)="Cancel" or normalize-space(.)="取消"]');
    await cancel.waitForDisplayed();
    await cancel.click();
    await browser.switchToWindow(mainHandle);
    sameBounds(await geometry('after-wizard-close'), normal);
    await stop();
    const saved = JSON.parse(await readFile(statePath, 'utf8'));
    assert.deepEqual(Object.keys(saved), ['main']);
    assert.equal(saved.main.width, normal.size.width);
    assert.equal(saved.main.height, normal.size.height);

    await launch();
    const restored = await geometry('restored-normal');
    sameBounds(restored, normal);
    assert.equal(restored.maximized, false);
    await browser.$('header nav a[href="/settings"]').click();
    await browser.$('button=English').waitForDisplayed();
    await browser.$('button=English').click();
    await browser.$('button[aria-label="Proxy"]').click();
    const connection = await browser.$('button[aria-label="HTTP request connection method"]');
    await connection.waitForDisplayed();
    await browser.execute(() => document.querySelector('button[aria-label="HTTP request connection method"]').focus());
    await browser.keys('Enter');
    await browser.$('[role="listbox"]').waitForDisplayed();
    await browser.execute(() => [...document.querySelectorAll('[role="option"]')]
      .find(option => option.textContent.trim().startsWith('Use proxy')).focus());
    await browser.keys('Enter');
    await browser.$('input[aria-label="HTTP request proxy server URL"]').waitForDisplayed();
    await invoke('plugin:window|close', { label: 'main' });
    await browser.$('[role="alertdialog"]').waitForDisplayed();
    assert.equal(child.exitCode, null);
    await browser.saveScreenshot(path.join(run.directory, 'window-state-close-guard.png'));
    await browser.$('button=Stay on this page').click();
    await browser.$('[role="alertdialog"]').waitForDisplayed({ reverse: true });
    await browser.$('button=Discard changes').scrollIntoView();
    await browser.$('button=Discard changes').click();
    await browser.$('header nav a[href="/"]').click();
    sameBounds(await geometry('close-cancelled-with-draft'), normal);
    await browser.maximizeWindow();
    await browser.waitUntil(() => invoke('plugin:window|is_maximized', { label: 'main' }));
    await geometry('chosen-maximized');
    await stop();

    await launch();
    const maximized = await geometry('restored-maximized');
    assert.equal(maximized.maximized, true);
    await invoke('plugin:window|toggle_maximize', { label: 'main' });
    await browser.waitUntil(async () => !(await invoke('plugin:window|is_maximized', { label: 'main' })));
    sameBounds(await geometry('restored-normal-after-maximize'), normal);
    if (process.platform === 'win32') {
      await invoke('plugin:window|minimize', { label: 'main' });
      await browser.waitUntil(() => invoke('plugin:window|is_minimized', { label: 'main' }));
      await stop();
      await launch();
      const afterMinimize = await geometry('restored-after-minimize');
      assert.equal(afterMinimize.minimized, false);
      assert.equal(afterMinimize.visible, true);
      sameBounds(afterMinimize, normal);
    }
    await stop();

    const misplaced = JSON.parse(await readFile(statePath, 'utf8'));
    Object.assign(misplaced.main, { x: 30000, y: -30000, prev_x: 30000, prev_y: -30000, maximized: false });
    await writeFile(statePath, JSON.stringify(misplaced));
    await launch();
    const relocated = await geometry('restored-missing-monitor');
    assert.equal(relocated.visible, true);
    const area = relocated.monitor.workArea;
    assert.ok(relocated.position.x >= area.position.x - 2 && relocated.position.y >= area.position.y - 2);
    assert.ok(relocated.position.x < area.position.x + area.size.width);
    assert.ok(relocated.position.y < area.position.y + area.size.height);
    assert.deepEqual(relocated.size, normal.size);
    await browser.saveScreenshot(path.join(run.directory, 'window-state-offscreen.png'));
    await stop();

    await writeFile(statePath, '{broken');
    await launch();
    const fallback = await geometry('restored-corrupt');
    assert.equal(fallback.visible, true);
    assert.equal(fallback.maximized, false);
    assert.equal(fallback.minimized, false);
    await stop();
    console.log(`Window state: normal, maximized, normal restore, offscreen, corrupt-state and unsaved-close checks passed. Evidence: ${run.directory}`);
  } catch (error) {
    if (browser) {
      await browser.saveScreenshot(path.join(run.directory, 'window-state-failure.png')).catch(() => {});
      const failure = await browser.execute(() => ({ text: document.body.innerText, focused: document.activeElement?.outerHTML })).catch(() => null);
      await writeFile(path.join(run.directory, 'window-state-failure.json'), JSON.stringify(failure, null, 2));
    }
    throw error;
  } finally {
    if (browser) await browser.deleteSession().catch(() => {});
    if (child?.pid && child.exitCode === null) {
      const exited = new Promise(resolve => child.once('exit', resolve));
      child.kill();
      await exited;
    }
    child?.stdout.unpipe(log);
    child?.stderr.unpipe(log);
    child?.stdout.destroy();
    child?.stderr.destroy();
    log?.end();
  }
}
