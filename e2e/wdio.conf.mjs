import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { browser } from '@wdio/globals';

const output = process.env.SKILL_DECK_E2E_OUTPUT;
const binary = process.env.SKILL_DECK_E2E_BINARY;
if (!output || !binary || !process.env.SKILL_DECK_E2E_HOME) {
  throw new Error('Run desktop tests with pnpm test:desktop');
}

// 用隔离目录中的无效记录显示真实恢复入口，覆盖标题栏同时存在多项状态的布局。
const recoveryFixture = path.join(process.env.SKILL_DECK_E2E_HOME, 'recovery', 'operation-layout-fixture');
await mkdir(recoveryFixture, { recursive: true });
await writeFile(path.join(recoveryFixture, 'recovery.json'), 'layout-fixture');
await mkdir(path.join(process.env.SKILL_DECK_E2E_HOME, '.codex'), { recursive: true });

export const config = {
  runner: 'local',
  specs: [process.env.SKILL_DECK_E2E_SPEC === 'translation' ? './translation.spec.mjs' : './desktop.spec.mjs'],
  maxInstances: 1,
  capabilities: [{ browserName: 'tauri' }],
  services: [['@wdio/tauri-service', {
    appBinaryPath: binary,
    driverProvider: 'embedded',
    captureBackendLogs: true,
    captureFrontendLogs: true,
    logDir: output,
  }]],
  framework: 'mocha',
  reporters: ['spec'],
  logLevel: 'warn',
  outputDir: output,
  waitforTimeout: 15000,
  connectionRetryTimeout: 30000,
  connectionRetryCount: 0,
  mochaOpts: { timeout: 60000 },
  async afterTest(test, _context, { passed }) {
    if (!passed) {
      await mkdir(output, { recursive: true });
      await browser.saveScreenshot(path.join(output, `failed-${test.title.replace(/[^a-z0-9]+/gi, '-')}.png`));
      const geometry = await browser.execute(() => ({
        requested: window.__desktopViewport,
        window: { width: innerWidth, height: innerHeight, outerWidth, outerHeight, x: screenX, y: screenY, scale: devicePixelRatio },
        screen: { width: screen.width, height: screen.height, availableWidth: screen.availWidth, availableHeight: screen.availHeight },
        active: document.activeElement.id,
        layout: localStorage.getItem('skill-deck-layout'),
        panels: [...document.querySelectorAll('[data-slot="resizable-panel"]')].map(el => ({ id: el.id, width: el.getBoundingClientRect().width })),
      }));
      await writeFile(path.join(output, `failed-${test.title.replace(/[^a-z0-9]+/gi, '-')}.json`), JSON.stringify(geometry, null, 2));
    }
  },
};
