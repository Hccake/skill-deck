import { describe, expect, it } from 'vitest';
import en from '../locales/en.json';
import zhCN from '../locales/zh-CN.json';

const requiredKeys = [
  'description',
  'scopeLabel',
  'scopeCount',
  'pathDisplayMode',
  'relativePaths',
  'fullPaths',
  'mainDirectory',
  'linkMode',
  'copyMode',
  'copyWarning',
  'previewError',
  'executionError',
  'recoveryRequired',
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
    expect(zhCN.skills.deleteConfirm.linkMode).toBe('链接');
    expect(zhCN.skills.deleteConfirm.copyMode).toBe('副本');
    expect(en.skills.deleteConfirm.linkMode).toBe('Link');
    expect(en.skills.deleteConfirm.copyMode).toBe('Copy');
  });

  it('describes Skill directories as the deletion targets', () => {
    expect(zhCN.skills.deleteConfirm.description)
      .toBe('将删除“{{name}}”的通用 Skill 目录，以及各 Agent 下对应的 Skill 目录。');
    expect(zhCN.skills.deleteConfirm.description).not.toContain('Agent 接入');
    expect(en.skills.deleteConfirm.description)
      .toBe('This deletes the shared “{{name}}” Skill directory and its corresponding Skill directories under each Agent.');
  });

  it('briefly explains that independent copies are also deleted', () => {
    expect(zhCN.skills.deleteConfirm.copyWarning)
      .toBe('部分 Agent 目录中存在此 Skill 的独立副本，本次操作会将其一并删除。');
    expect(en.skills.deleteConfirm.copyWarning)
      .toBe('Some Agent directories contain independent copies of this Skill. This operation will delete them as well.');
  });

  it('explains removal recovery as an incomplete operation that needs file review', () => {
    expect(zhCN.skills.deleteConfirm.recoveryRequired)
      .toBe('删除未完成，相关文件需要检查。请先处理下方恢复项。');
    expect(en.skills.deleteConfirm.recoveryRequired)
      .toBe('Deletion did not finish. Some files need to be checked before continuing.');
  });

  it('describes restore failure as an unfinished user operation', () => {
    expect(zhCN.mutation.result.errors.restoreFailed)
      .toBe('Skill Deck 未能妥善完成这次操作，相关文件需要检查。');
    expect(zhCN.mutation.result.errors.restoreFailed).not.toContain('自动恢复');
    expect(en.mutation.result.errors.restoreFailed)
      .toBe('Skill Deck could not safely complete this operation. The related files need review.');
  });
});
