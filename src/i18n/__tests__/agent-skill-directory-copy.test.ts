import { describe, expect, it } from 'vitest';
import en from '../locales/en.json';
import zhCN from '../locales/zh-CN.json';

describe('Agent Skill directory copy', () => {
  it('describes Agent read locations with natural Chinese wording', () => {
    expect(zhCN.settings.agents.skillReading).toEqual({
      title: 'Skill 读取',
      readMethod: '读取规则',
    });
    expect(zhCN.settings.agents.installDetection).toEqual({
      title: '安装检测',
      cardHint: '任一位置存在',
      hint: '任一检测路径存在，即视为已安装。',
    });
    expect(zhCN.settings.agents.global).toMatchObject({
      title: 'Global',
      readTitle: 'Global Skill 读取',
      enabled: '启用 Global Skill 读取',
      location: 'Global 读取位置',
    });
    expect(zhCN.settings.agents.project).toMatchObject({
      title: 'Project',
      readTitle: 'Project Skill 读取',
      enabled: '启用 Project Skill 读取',
      location: 'Project 读取位置',
    });
    expect(zhCN.settings.agents.locations).toEqual({
      shared: '仅读取通用 Skill 目录',
      private: '仅读取 Agent 专用目录',
      both: '两者都读取',
    });
    expect(zhCN.settings.agents.readMode.shared).toBe('从通用 Skill 目录读取');
    expect(zhCN.settings.agents.readMode.private).toBe('从此 Agent 的 Skill 目录读取');
    expect(zhCN.settings.agents.readMode.both).toBe('同时从以上两个位置读取');
    expect(en.settings.agents.locations.both).toBe('Both directories');
    expect(zhCN.settings.agents.directoryKind).toEqual({
      shared: '通用 Skill 目录',
      private: 'Agent 专用 Skill 目录',
    });
    expect(zhCN.settings.agents.pathLocations).toEqual({
      home: '用户主目录',
      configHome: '用户配置目录',
      project: '项目目录',
      absolute: '绝对路径',
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
    expect(zhCN.settings.agents.delete).toBe('删除');
    expect(en.settings.agents.delete).toBe('Delete');
    expect(zhCN.settings.agents).not.toHaveProperty('duplicate');
    expect(en.settings.agents).not.toHaveProperty('duplicate');
    expect(zhCN.settings.agents.form.title).not.toHaveProperty('duplicate');
    expect(en.settings.agents.form.title).not.toHaveProperty('duplicate');
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
