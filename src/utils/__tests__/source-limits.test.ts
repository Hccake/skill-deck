import { beforeAll, describe, expect, it } from 'vitest';
import { createInstance } from 'i18next';
import zhCN from '@/i18n/locales/zh-CN.json';
import { formatAppError } from '@/utils/format-app-error';

describe('source support guidance', () => {
  const i18n = createInstance();
  beforeAll(async () => { await i18n.init({ lng: 'zh-CN', resources: { 'zh-CN': { translation: zhCN } } }); });
  it.each([
    ['sourceDirectoryLinks', '真实目录'],
    ['sourceSessionCapacity', '较小的来源'],
    ['payloadSessionCapacity', '减少选择'],
  ])('explains the next step for %s', (capability, guidance) => {
    const message = formatAppError({ kind: 'capabilityUnavailable', data: { capability, path: null } }, i18n.getFixedT('zh-CN'));
    expect(message).toContain(guidance);
  });
});
