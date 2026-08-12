import { describe, expect, it } from 'vitest';
import en from '../locales/en.json';
import zhCN from '../locales/zh-CN.json';

function serialized(value: unknown): string {
  return JSON.stringify(value);
}

describe('product language boundary', () => {
  it('keeps internal Agent installation nouns out of user-facing copy', () => {
    const zhProductCopy = serialized({
      agentSelection: zhCN.agentSelection,
      skills: zhCN.skills,
      addSkill: zhCN.addSkill,
    });
    const enProductCopy = serialized({
      agentSelection: en.agentSelection,
      skills: en.skills,
      addSkill: en.addSkill,
    });

    expect(zhProductCopy).not.toMatch(/Agent 专用(?:安装项|目录项)/);
    expect(zhProductCopy).not.toMatch(/主 Skill|写入方式/);
    expect(enProductCopy).not.toMatch(/Agent-specific installation|setup items?|separate setup/i);
    expect(enProductCopy).not.toMatch(/main Skill|write method/i);
  });

  it('does not expose internal Skill location and directory model names', () => {
    expect(serialized(zhCN)).not.toContain('使用范围');
    expect(serialized(en)).not.toMatch(/Usage Scope|standard Skill director|This computer/i);
    expect(serialized(zhCN.settings.agents)).not.toContain('Agent 定义');
    expect(serialized(en.settings.agents)).not.toMatch(/Agent definitions?|Custom Agents?/i);
  });

  it('uses task language for installation, update, and duplicate-copy guidance', () => {
    expect(zhCN.addSkill.scopeSelect).toMatchObject({
      title: '选择 Skill 位置',
      hint: '选择安装到全局位置或具体 Project。',
    });
    expect(zhCN.addSkill.confirm).toMatchObject({
      defaultLocation: '安装到通用 Skill 目录',
      createLinks: '将创建链接',
      createCopies: '将创建副本',
    });
    expect(zhCN.skills.updatePlan).toMatchObject({
      standardSkillAction: '更新通用 Skill 目录中的 Skill',
      cleanCopiesAction: '同步 {{count}} 个未修改副本',
      adapterTargetsAction: '同步 {{agents}} 使用的项目内 Skill',
      conflictingCopies: '发现已修改的副本',
    });
    expect(zhCN.skills.card.extraCopies).toBe('部分 Agent 的目录中还保留链接或副本');
    expect(zhCN.settings.agents).toMatchObject({
      add: '添加 Agent 信息',
      description: '查看各 Agent 会读取哪些 Skill 目录，以及 Skill Deck 用来检测它们的位置；也可以补充尚未收录的 Agent 信息。',
    });
    expect(en.settings.agents).toMatchObject({
      add: 'Add Agent information',
      description: 'Review which Skill directories each Agent reads and where Skill Deck detects it. You can also add information for Agents that are not yet included.',
    });
  });

  it('names the WSL setting as product support rather than an integration', () => {
    expect(zhCN.settings.general).toMatchObject({
      description: '管理外观、语言和平台功能。',
      wslTitle: 'WSL 支持',
      wslSaving: '正在更新 WSL 设置',
      wslSaveError: '无法更新 WSL 设置，请重试。',
      wslDisableTitle: '关闭 WSL 支持？',
      wslDisableOnlyConfirm: '关闭 WSL 支持',
      wslDisabling: '正在关闭 WSL 支持',
    });
    expect(en.settings.general).toMatchObject({
      description: 'Manage appearance, language, and platform features.',
      wslTitle: 'WSL support',
      wslSaving: 'Updating WSL settings',
      wslSaveError: 'WSL settings could not be updated. Try again.',
      wslDisableTitle: 'Disable WSL support?',
      wslDisableOnlyConfirm: 'Disable WSL support',
      wslDisabling: 'Disabling WSL support',
    });
    expect(serialized(zhCN.settings.general)).not.toContain('WSL 集成');
    expect(serialized(en.settings.general)).not.toMatch(/WSL integration/i);
  });

});
