import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createServer } from 'node:net';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { homedir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { assertDriverPortAvailable, createDesktopRun, desktopPaths, tauriFrontendDist } from '../desktop-e2e.mjs';

test('Tauri receives filesystem paths rather than drive-letter URLs', () => {
  for (const directory of ['D:\\Desktop tests\\dist', '\\\\server\\share\\dist', '\\\\?\\C:\\cache\\dist']) {
    const result = tauriFrontendDist(directory, 'win32');
    assert.equal(URL.canParse(result), false);
    assert.equal(result, path.win32.toNamespacedPath(directory));
  }
  assert.equal(tauriFrontendDist('/home/test/desktop/dist', 'linux'), '/home/test/desktop/dist');
});

test('an occupied driver port is rejected before launching an application', async () => {
  const server = createServer();
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port;
  try {
    await assert.rejects(assertDriverPortAvailable(port), /EADDRINUSE/);
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
  await assertDriverPortAvailable(port);
  await assert.rejects(assertDriverPortAvailable(NaN), /Invalid WebDriver port/);
});

test('desktop automation is opt-in and absent from production capabilities', async () => {
  const repository = fileURLToPath(new URL('../../', import.meta.url));
  const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1', '--manifest-path', 'src-tauri/Cargo.toml'], { cwd: repository, encoding: 'utf8' }));
  const app = metadata.packages.find((entry) => entry.name === 'app');
  assert.ok(!app.features.default?.includes('desktop-e2e'));
  for (const name of ['tauri-plugin-wdio', 'tauri-plugin-wdio-webdriver']) {
    assert.equal(app.dependencies.find((dependency) => dependency.name === name).optional, true);
  }
  const production = JSON.parse(await readFile(path.join(repository, 'src-tauri/tauri.conf.json'), 'utf8'));
  const desktop = JSON.parse(await readFile(path.join(repository, 'e2e/tauri.e2e.conf.json'), 'utf8'));
  assert.notEqual(production.identifier, desktop.identifier);
  for (const name of ['main', 'install-wizard']) {
    const capability = JSON.parse(await readFile(path.join(repository, 'src-tauri/capabilities', `${name}.json`), 'utf8'));
    assert.ok(capability.permissions.every((permission) => !JSON.stringify(permission).includes('wdio:')));
  }
});

test('desktop paths use a separate target and platform executable', () => {
  const root = path.join(homedir(), '.cache', 'skill-deck-validation', 'desktop-tests');
  for (const platform of ['linux', 'darwin', 'win32']) {
    const paths = desktopPaths(root, platform);
    assert.equal(paths.binary, path.join(root, 'target', 'debug', platform === 'win32' ? 'app.exe' : 'app'));
    assert.equal(paths.frontend, path.join(root, 'dist'));
  }
});

test('each run isolates application state and retains a separate evidence directory', async () => {
  const root = await mkdtemp(path.join(homedir(), '.skill-deck-e2e-test-'));
  try {
    const first = await createDesktopRun(root);
    const second = await createDesktopRun(root);
    assert.notEqual(first.directory, second.directory);
    assert.equal(first.env.SKILL_DECK_E2E_HOME, path.join(first.directory, 'home'));
    assert.equal(first.env.SKILL_DECK_BINDINGS_OUT, path.join(first.directory, 'bindings.ts'));
    assert.equal(first.env.WEBVIEW2_USER_DATA_FOLDER, path.join(first.directory, 'webview'));
    assert.equal(first.env.CODEX_HOME, path.join(first.env.SKILL_DECK_E2E_HOME, '.codex'));
    assert.equal(first.env.XDG_STATE_HOME, path.join(first.directory, 'state'));
    assert.equal(JSON.parse(await readFile(path.join(first.directory, 'home', '.skill-deck', 'config.json'), 'utf8')).wslIntegrationEnabled, false);
    assert.ok(first.directory.startsWith(root + path.sep));
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
