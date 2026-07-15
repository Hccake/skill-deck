import { describe, expect, it } from 'vitest';
import en from '../locales/en.json';
import zhCN from '../locales/zh-CN.json';

describe.each([
  ['en', en],
  ['zh-CN', zhCN],
])('%s lifecycle interruption copy', (_locale, messages) => {
  it.each([
    'closeWindowTitle',
    'closeWindowDescription',
    'closeWindowWaitDescription',
    'quitTitle',
    'quitDescription',
    'quitWaitDescription',
    'restartTitle',
    'restartDescription',
    'restartWaitDescription',
    'cancelAndCloseWindow',
    'cancelAndQuit',
    'cancelAndRestart',
  ])('defines %s', (key) => {
    const interruption = messages.mutation.interruption as Record<string, string>;
    expect(interruption[key]).toBeTruthy();
  });

  it('does not retain the ambiguous legacy close keys', () => {
    const interruption = messages.mutation.interruption as Record<string, string>;
    expect(interruption.closeTitle).toBeUndefined();
    expect(interruption.closeDescription).toBeUndefined();
    expect(interruption.closeWaitDescription).toBeUndefined();
    expect(interruption.cancelAndClose).toBeUndefined();
  });
});
