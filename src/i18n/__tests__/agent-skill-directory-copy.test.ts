import { describe, expect, it } from 'vitest';
import en from '../locales/en.json';
import zhCN from '../locales/zh-CN.json';

describe('Agent Skill directory copy', () => {
  it('describes Agent read locations with natural Chinese wording', () => {
    expect(zhCN.settings.agents.skillReading).toEqual({
      title: 'Skill 读取目录',
      readMethod: '读取位置',
    });
    expect(zhCN.settings.agents.installDetection).toEqual({
      title: 'Agent 安装检测',
      hint: '任一路径存在时，Skill Deck 会认为当前 Environment 已安装此 Agent。',
    });
    expect(zhCN.settings.agents.global).toMatchObject({
      title: 'Global',
      readTitle: 'Global',
      enabled: '启用 Global',
      location: 'Global 读取位置',
    });
    expect(zhCN.settings.agents.project).toMatchObject({
      title: 'Project',
      readTitle: 'Project',
      enabled: '启用 Project',
      location: 'Project 读取位置',
    });
    expect(zhCN.settings.agents.locations).toEqual({
      shared: '仅读取通用 Skill 目录',
      private: '仅读取此 Agent 的 Skill 目录',
      both: '同时读取两个目录',
    });
    expect(zhCN.settings.agents.readMode.shared).toBe('从通用 Skill 目录读取');
    expect(zhCN.settings.agents.readMode.private).toBe('从此 Agent 的 Skill 目录读取');
    expect(zhCN.settings.agents.readMode.both).toBe('同时从以上两个位置读取');
    expect(zhCN.settings.agents.directoryKind).toEqual({
      shared: '通用 Skill 目录',
      private: '此 Agent 的 Skill 目录',
    });
    expect(zhCN.settings.agents.sharedDirectories).toEqual({
      title: '通用 Skill 目录',
      cardLabel: '通用目录',
      sharedAriaLabel: '{{scope}}：读取通用 Skill 目录',
      privateAriaLabel: '{{scope}}：读取此 Agent 的 Skill 目录',
      bothAriaLabel: '{{scope}}：同时读取通用 Skill 目录和此 Agent 的 Skill 目录',
    });
    expect(en.settings.agents.sharedDirectories).toEqual({
      title: 'Shared Skill directories',
      cardLabel: 'Shared directory',
      sharedAriaLabel: '{{scope}}: reads the shared Skill directory',
      privateAriaLabel: '{{scope}}: reads this Agent\'s Skill directory',
      bothAriaLabel: '{{scope}}: reads the shared Skill directory and this Agent\'s Skill directory',
    });
    expect(zhCN.settings.agents.duplicate).toBe('复制');
    expect(zhCN.settings.agents.delete).toBe('删除');
    expect(en.settings.agents.duplicate).toBe('Duplicate');
    expect(en.settings.agents.delete).toBe('Delete');
    expect(zhCN.settings.agents.detection).toMatchObject({
      cardLabel: '检测',
      cardTooltip: 'Agent 安装检测路径',
    });
    expect(en.settings.agents.detection).toMatchObject({
      cardLabel: 'Detection',
      cardTooltip: 'Agent installation detection paths',
    });
  });

  it('does not expose rejected directory categories in Agent settings and selection copy', () => {
    const copy = JSON.stringify({
      settings: {
        agents: zhCN.settings.agents,
        installPreferences: zhCN.settings.installPreferences,
      },
      addSkillAgents: zhCN.addSkill.agents,
    });

    expect(copy).not.toContain('共享目录');
    expect(copy).not.toContain('Agent 独立目录');
    expect(copy).not.toContain('独立 Skill');
    expect(zhCN.addSkill.agents.additionalHint).toBe(
      '为只从自身 Skill 目录读取的 Agent 创建链接或副本'
    );
    expect(zhCN.addSkill.agents.privateRequiredHint).toBe(
      '这些 Agent 只从自己的 Skill 目录读取。选择后，安装时会创建链接或副本。'
    );
  });
});
