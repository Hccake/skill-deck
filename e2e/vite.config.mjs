import { readFileSync } from 'node:fs';
import { mergeConfig } from 'vite';
import base from '../vite.config';

const config = mergeConfig(base, {
  plugins: [{
    name: 'desktop-e2e-bridge',
    enforce: 'pre',
    transform(code, id) {
      if (process.env.SKILL_DECK_E2E_SPEC === 'download-maintenance' && id.replaceAll('\\', '/').endsWith('/src/pages/WizardPage.tsx')) {
        if (!code.includes('createInitialState(wizardParams)')) throw new Error('Wizard fixture initialization anchor changed');
        return code.replace('createInitialState(wizardParams)', 'window.__downloadFixture.wizardState')
          .replace('<div className="flex-shrink-0 h-[72px] border-t', '<div data-e2e-wizard-footer className="flex-shrink-0 h-[72px] border-t');
      }
      if (process.env.SKILL_DECK_E2E_SPEC === 'translation' && id.replaceAll('\\', '/').endsWith('/src/lib/discover/api.ts')) {
        return { code: readFileSync(new URL('./fixtures/translation-discovery.mjs', import.meta.url), 'utf8'), map: null };
      }
      return null;
    },
    transformIndexHtml: {
      order: 'pre',
      handler: html => ({
        html: ['copy-dialog', 'update-relocation', 'source-filter', 'basic-compatibility', 'download-maintenance'].includes(process.env.SKILL_DECK_E2E_SPEC)
          ? html.replace('/src/main.tsx', `/e2e/fixtures/${process.env.SKILL_DECK_E2E_SPEC}.tsx`) : html,
        tags: [{ tag: 'script', attrs: { type: 'module' }, children: "import '@wdio/tauri-plugin';", injectTo: 'head' }],
      }),
    },
  }],
});

if (process.env.SKILL_DECK_E2E_SPEC === 'download-maintenance') {
  config.plugins = [config.plugins.at(-1), ...config.plugins.slice(0, -1)];
}
export default config;
