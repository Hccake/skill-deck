import { describe, expect, it } from 'vitest';
import en from '../locales/en.json';
import zhCN from '../locales/zh-CN.json';

const requiredKeys = [
  'agentEntriesSection',
  'linkMode',
  'copyMode',
  'noAgentEntries',
  'copyWarning',
  'previewError',
  'executionError',
  'stale',
  'retryPreview',
  'retryDelete',
] as const;

describe('Skill deletion copy', () => {
  it('defines the complete deletion dialog copy in both locales', () => {
    for (const locale of [en, zhCN]) {
      const messages = locale.skills.deleteConfirm as Record<string, string>;
      for (const key of requiredKeys) {
        expect(messages[key]).toEqual(expect.any(String));
        expect(messages[key].length).toBeGreaterThan(0);
      }
      expect(JSON.stringify(messages).toLowerCase()).not.toContain('junction');
    }
  });

  it('uses user-facing install method names', () => {
    expect(zhCN.skills.deleteConfirm.linkMode).toBe('软连接');
    expect(zhCN.skills.deleteConfirm.copyMode).toBe('副本');
    expect(en.skills.deleteConfirm.linkMode).toBe('Symbolic link');
    expect(en.skills.deleteConfirm.copyMode).toBe('Copy');
  });
});
