import { mkdir } from 'node:fs/promises';
import path from 'node:path';
import { browser } from '@wdio/globals';

const output = process.env.SKILL_DECK_E2E_OUTPUT;
const binary = process.env.SKILL_DECK_E2E_BINARY;
if (!output || !binary || !process.env.SKILL_DECK_E2E_HOME) {
  throw new Error('Run desktop tests with pnpm test:desktop');
}

export const config = {
  runner: 'local',
  specs: ['./desktop.spec.mjs'],
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
    }
  },
};
