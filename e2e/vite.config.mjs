import { mergeConfig } from 'vite';
import base from '../vite.config';

export default mergeConfig(base, {
  plugins: [{
    name: 'desktop-e2e-bridge',
    transformIndexHtml: {
      order: 'pre',
      handler: () => [{ tag: 'script', attrs: { type: 'module' }, children: "import '@wdio/tauri-plugin';", injectTo: 'head' }],
    },
  }],
});
