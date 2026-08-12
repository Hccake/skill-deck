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
      title: 'Agent 检测',
      cardHint: '任一位置存在',
      hint: '任一 Agent 检测位置存在时，即视为已检测到。',
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
      standard: '仅读取通用 Skill 目录',
      private: '仅读取 Agent 专用目录',
      both: '两者都读取',
    });
    expect(zhCN.settings.agents.readMode.standard).toBe('从通用 Skill 目录读取');
    expect(zhCN.settings.agents.readMode.private).toBe('从此 Agent 的 Skill 目录读取');
    expect(zhCN.settings.agents.readMode.both).toBe('同时从以上两个位置读取');
    expect(en.settings.agents.locations.both).toBe('Both directories');
    expect(zhCN.settings.agents.directoryKind).toEqual({
      standard: '通用 Skill 目录',
      private: 'Agent 专用 Skill 目录',
    });
    expect(zhCN.settings.agents.pathLocations).toEqual({
      home: '用户主目录',
      configHome: '用户配置目录',
      project: '项目目录',
      absolute: '绝对路径',
    });
    expect(zhCN.settings.agents.standardDirectories).toEqual({
      title: '通用 Skill 目录',
      cardLabel: '通用目录',
      standardAriaLabel: '{{scope}}：读取通用 Skill 目录',
      privateAriaLabel: '{{scope}}：读取此 Agent 的 Skill 目录',
      bothAriaLabel: '{{scope}}：同时读取通用 Skill 目录和此 Agent 的 Skill 目录',
    });
    expect(en.settings.agents.standardDirectories).toEqual({
      title: 'Common Skill directories',
      cardLabel: 'Common directory',
      standardAriaLabel: '{{scope}}: reads the common Skill directory',
      privateAriaLabel: '{{scope}}: reads this Agent\'s Skill directory',
      bothAriaLabel: '{{scope}}: reads the common Skill directory and this Agent\'s Skill directory',
    });
    expect(zhCN.settings.agents.delete).toBe('删除');
    expect(en.settings.agents.delete).toBe('Delete');
    expect(zhCN.settings.agents.detection).toMatchObject({
      cardLabel: '检测',
      cardTooltip: 'Agent 检测位置',
    });
    expect(en.settings.agents.detection).toMatchObject({
      cardLabel: 'Detection',
      cardTooltip: 'Agent detection locations',
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
  });
});
