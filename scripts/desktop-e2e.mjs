import { spawn } from 'node:child_process';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { createServer } from 'node:net';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const repository = fileURLToPath(new URL('../', import.meta.url));

export async function assertDriverPortAvailable(port) {
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error('Invalid WebDriver port');
  const server = createServer();
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(port, '127.0.0.1', resolve);
  });
  await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
}

export function desktopPaths(root, platform = process.platform) {
  return {
    target: path.join(root, 'target'),
    frontend: path.join(root, 'dist'),
    binary: path.join(root, 'target', 'debug', platform === 'win32' ? 'app.exe' : 'app'),
  };
}

export function tauriFrontendDist(directory, platform = process.platform) {
  // Tauri 先尝试解析 URL；逐字路径避免 Windows 盘符被识别为 URL scheme。
  return platform === 'win32' ? path.win32.toNamespacedPath(directory) : directory;
}

export async function createDesktopRun(root) {
  await mkdir(path.join(root, 'runs'), { recursive: true });
  const directory = await mkdtemp(path.join(root, 'runs', `${process.platform}-`));
  const home = path.join(directory, 'home');
  await mkdir(path.join(home, '.skill-deck'), { recursive: true });
  await writeFile(path.join(home, '.skill-deck', 'config.json'), JSON.stringify({ wslIntegrationEnabled: false }));
  return {
    directory,
    env: {
      SKILL_DECK_E2E_HOME: home,
      SKILL_DECK_E2E_OUTPUT: directory,
      SKILL_DECK_BINDINGS_OUT: path.join(directory, 'bindings.ts'),
      WEBVIEW2_USER_DATA_FOLDER: path.join(directory, 'webview'),
      XDG_CONFIG_HOME: path.join(home, '.config'),
      XDG_DATA_HOME: path.join(directory, 'data'),
      XDG_CACHE_HOME: path.join(directory, 'cache'),
      XDG_STATE_HOME: path.join(directory, 'state'),
      CODEX_HOME: path.join(home, '.codex'),
      CLAUDE_CONFIG_DIR: path.join(home, '.claude'),
      APPDATA: path.join(home, 'AppData', 'Roaming'),
      LOCALAPPDATA: path.join(home, 'AppData', 'Local'),
    },
  };
}

async function runNode(script, args, env) {
  const child = spawn(process.execPath, [script, ...args], {
    cwd: repository, env, stdio: 'inherit', detached: process.platform !== 'win32',
  });
  let interrupted = false;
  const stop = () => {
    interrupted = true;
    if (!child.pid) return;
    if (process.platform === 'win32') {
      spawn('taskkill', ['/pid', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
    } else {
      try { process.kill(-child.pid, 'SIGTERM'); } catch (error) { if (error.code !== 'ESRCH') throw error; }
    }
  };
  process.once('SIGINT', stop);
  process.once('SIGTERM', stop);
  try {
    await new Promise((resolve, reject) => {
      child.once('error', reject);
      child.once('exit', (code, signal) => code === 0 && !interrupted ? resolve() : reject(new Error(`${path.basename(script)} failed (${signal ?? code})`)));
    });
  } finally {
    process.removeListener('SIGINT', stop);
    process.removeListener('SIGTERM', stop);
  }
}

async function main() {
  const root = path.resolve(process.env.SKILL_DECK_DESKTOP_ROOT ?? path.join(homedir(), '.cache', 'skill-deck-validation', 'desktop'));
  const paths = desktopPaths(root);
  const run = await createDesktopRun(root);
  console.log(`Desktop evidence: ${run.directory}`);
  const env = { ...process.env, ...run.env, CARGO_TARGET_DIR: paths.target, SKILL_DECK_E2E_BINARY: paths.binary };
  if (!process.argv.includes('--no-build')) {
    await runNode(path.join(repository, 'node_modules/vite/bin/vite.js'), ['build', '--config', 'e2e/vite.config.mjs', '--outDir', paths.frontend, '--emptyOutDir'], process.env);
    const config = JSON.parse(await readFile(path.join(repository, 'e2e/tauri.e2e.conf.json'), 'utf8'));
    config.build = { beforeBuildCommand: '', frontendDist: tauriFrontendDist(paths.frontend) };
    await runNode(path.join(repository, 'node_modules/@tauri-apps/cli/tauri.js'), [
      'build', '--debug', '--no-bundle', '--features', 'desktop-e2e', '--config', JSON.stringify(config), '--', '--locked',
    ], env);
  }
  const port = Number(env.TAURI_WEBDRIVER_PORT ?? 4445);
  await assertDriverPortAvailable(port);
  if (process.argv.includes('--window-state')) {
    const { verifyWindowState } = await import('../e2e/window-state.mjs');
    await verifyWindowState(paths.binary, run, port);
  } else {
    await runNode(path.join(repository, 'node_modules/@wdio/cli/bin/wdio.js'), ['run', 'e2e/wdio.conf.mjs'], env);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().catch((error) => { console.error(error); process.exitCode = 1; });
}
