import { describe, expect, it } from 'vitest';
import zhCN from '../locales/zh-CN.json';
import en from '../locales/en.json';

describe('add skill copy', () => {
  it('uses precise Chinese wording for target conflicts and agent detection', () => {
    expect(zhCN.addSkill.agents.detected).toBe('已检测到');
    expect(zhCN.addSkill.confirm.overwriteGroup).toBe('目标目录已存在');
    expect(zhCN.addSkill.confirm.summary).toBe(
      '将安装 {{count}} 个 Skill，其中 {{overwriteCount}} 个目标目录已有同名 Skill，继续后会覆盖对应内容'
    );
    expect(zhCN.addSkill.confirm.overwriteCount).toBe(
      '{{count}} 个目标目录已有同名 Skill，继续后会覆盖对应内容'
    );
    expect(zhCN.addSkill.confirm.conflictZone).toBe('已有目标目录');
    expect(zhCN.addSkill.confirm.freshZone).toBe('新增写入');
    expect(zhCN.addSkill.actions.installWithOverwrite).toBe('确认覆盖并继续');
    expect(zhCN.addSkill.complete.skippedCategory).toBe('未写入');
    expect(zhCN.addSkill.complete.skipped).toBe('未写入: {{agents}}');
  });

  it('keeps the English fallback aligned with the same product semantics', () => {
    expect(en.addSkill.agents.detected).toBe('Detected');
    expect(en.addSkill.confirm.overwriteGroup).toBe('Target exists');
    expect(en.addSkill.confirm.summary).toBe(
      'Will install {{count}} skills. {{overwriteCount}} target directories already contain matching skills and will be overwritten.'
    );
    expect(en.addSkill.confirm.overwriteCount).toBe(
      '{{count}} target directories already contain matching skills and will be overwritten'
    );
    expect(en.addSkill.confirm.conflictZone).toBe('Existing targets');
    expect(en.addSkill.confirm.freshZone).toBe('New writes');
    expect(en.addSkill.actions.installWithOverwrite).toBe('Confirm overwrite and continue');
    expect(en.addSkill.complete.skippedCategory).toBe('Not written');
    expect(en.addSkill.complete.skipped).toBe('Not written: {{agents}}');
  });
});

describe('copy to project copy', () => {
  it('keeps source maintenance messaging out of the Chinese copy flow', () => {
    expect('metadataWarning' in zhCN.skills.copyToProject).toBe(false);
    expect('sourceRepairRequired' in zhCN.skills.copyToProject).toBe(false);
    expect('repairSource' in zhCN.skills.copyToProject).toBe(false);
  });

  it('keeps source maintenance messaging out of the English copy flow', () => {
    expect('metadataWarning' in en.skills.copyToProject).toBe(false);
    expect('sourceRepairRequired' in en.skills.copyToProject).toBe(false);
    expect('repairSource' in en.skills.copyToProject).toBe(false);
  });
});
