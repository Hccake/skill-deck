import { readFileSync } from 'node:fs';
import { mergeConfig } from 'vite';
import base from '../vite.config';

export default mergeConfig(base, {
  plugins: [{
    name: 'desktop-e2e-bridge',
    transform(code, id) {
      if (process.env.SKILL_DECK_E2E_SPEC === 'translation' && id.replaceAll('\\', '/').endsWith('/src/lib/discover/api.ts')) {
        return { code: readFileSync(new URL('./fixtures/translation-discovery.mjs', import.meta.url), 'utf8'), map: null };
      }
      return null;
    },
    transformIndexHtml: {
      order: 'pre',
      handler: html => ({
        html: ['copy-dialog', 'update-relocation'].includes(process.env.SKILL_DECK_E2E_SPEC)
          ? html.replace('/src/main.tsx', `/e2e/fixtures/${process.env.SKILL_DECK_E2E_SPEC}.tsx`) : html,
        tags: [{ tag: 'script', attrs: { type: 'module' }, children: "import '@wdio/tauri-plugin';", injectTo: 'head' }],
      }),
    },
  }],
});
