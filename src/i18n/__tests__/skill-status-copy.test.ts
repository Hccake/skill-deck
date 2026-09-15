import { describe, expect, it } from 'vitest';
import zh from '../locales/zh-CN.json';
import en from '../locales/en.json';

describe('Skill status copy', () => {
  it('uses explicit version availability and keeps completion distinct from up-to-date', () => {
    expect(zh.skills.updateStatusLabel.available).toBe('新版本可用');
    expect(en.skills.updateStatusLabel.available).toBe('New version available');
    expect(zh.skills.checkUpToDate).toBe('已是最新');
    expect(zh.skills.checkCompleted).toBe('检查完成');
  });

  it('does not prescribe a GitHub token for generic authentication issues', () => {
    for (const locale of [zh, en]) {
      for (const value of [locale.skills.updateEvidence.nextStep.configureToken,
        locale.skills.updateEvidence.nextStep.configureTokenOrWait,
        locale.skills.updateEvidence.nextStep.checkSourceOrToken,
        locale.skills.updateEvidence.actions.configureToken, locale.skills.updateHint.auth]) {
        expect(value).not.toMatch(/github|github_token/i);
      }
    }
  });

  it('does not advise retrying an unknown check failure', () => {
    expect(zh.skills.checkUpdatesFailed).toBe('更新检查未完成。');
    expect(en.skills.checkUpdatesFailed).toBe('Update check did not complete.');
    expect(zh.skills.updateHint.missingRemoteHashCanUpdate).toBe('缺少本地版本信息，无法判断是否有新版本。仍可从原来源更新。');
  });
});
