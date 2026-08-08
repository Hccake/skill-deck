import { describe, expect, it } from 'vitest';
import en from '../locales/en.json';
import zhCN from '../locales/zh-CN.json';

describe('Agent selection copy', () => {
  it('keeps installation-mode wording in the Agent selection namespace', () => {
    expect(zhCN.agentSelection.linkRecommended).toBe('链接（推荐）');
    expect(zhCN.agentSelection.copyOnly).toBe('仅支持复制');
    expect(zhCN.agentSelection.modeTitle).toBe('安装方式');
    expect(zhCN.agentSelection.modeHelp)
      .toBe('决定如何将 Skill 安装到所选 Agent 的 Skill 目录；仅支持复制的 Agent 不受此设置影响。');
    expect(en.agentSelection.linkRecommended).toBe('Link (recommended)');
    expect(en.agentSelection.copyOnly).toBe('Copy only');
    expect(en.agentSelection.modeTitle).toBe('Installation method');
    expect(en.agentSelection.modeHelp)
      .toBe('Choose how to install this Skill in the selected Agent directories. Copy-only Agents are not affected.');
  });

  it('identifies the managed Skill in the dialog title', () => {
    expect(zhCN.skills.manageAgents.title).toBe('管理「{{name}}」的关联 Agent');
    expect(en.skills.manageAgents.title).toBe('Manage Agents Linked to “{{name}}”');
  });

  it('distinguishes selectable Agents from the read-only overflow summary', () => {
    expect(zhCN.agentSelection.otherAgents).toBe('其他可选 Agent（{{count}}）');
    expect(en.agentSelection.otherAgents).toBe('Other selectable Agents ({{count}})');
    expect(zhCN.agentSelection.moreAgents).toBe('+ 其他 {{count}} 个');
    expect(en.agentSelection.moreAgents).toBe('+ {{count}} more');
    expect(zhCN.agentSelection.moreAgentsDescription)
      .toBe('这些 Agent 也会读取通用 Skill 目录，但当前检测状态未确认。');
    expect(zhCN.agentSelection.memberCount).toBe('{{count}} 个 Agent');
    expect(zhCN.agentSelection.viewMembers).toBe('查看成员');
    expect(zhCN.agentSelection.sharedPlacementDescription)
      .toBe('这些 Agent 使用同一个 Skill 目录，选择后将统一处理。');
    expect(zhCN.agentSelection.detection.detected).toBe('已检测到');
    expect(zhCN.agentSelection.detectedCount).toBe('{{detected}}/{{total}} 已检测到');
  });

  it('uses scenario-specific headings and guidance without changing the selection model', () => {
    expect(zhCN.agentSelection.automatic.install.title).toBe('安装后可直接使用');
    expect(zhCN.agentSelection.automatic.install.help)
      .toBe('Skill 会安装到通用 Skill 目录，这些 Agent 可以直接从该目录读取，无需额外设置。');
    expect(zhCN.agentSelection.automatic.manage.title).toBe('无需选择即可使用');
    expect(zhCN.agentSelection.automatic.manage.help)
      .toBe('此 Skill 已安装在通用 Skill 目录，这些 Agent 可以直接读取，无需选择。');
    expect(zhCN.agentSelection.automatic.copyToProject.title).toBe('复制后可直接使用');
    expect(zhCN.agentSelection.automatic.copyToProject.help)
      .toBe('Skill 会复制到目标 Project 的通用 Skill 目录，这些 Agent 可以直接读取，无需额外设置。');
    expect(zhCN.agentSelection.selectable.title).toBe('选择后可使用');
    expect(zhCN.agentSelection.selectable.help)
      .toBe('这些 Agent 不读取通用 Skill 目录。选择后，Skill Deck 会在其 Skill 目录中创建链接或副本。');
  });

  it('presents own-directory installations as an optional nested setting', () => {
    expect(zhCN.agentSelection.ownDirectory.title)
      .toBe('同时安装到 Agent 自己的 Skill 目录（可选）');
    expect(zhCN.agentSelection.ownDirectory.install.description)
      .toBe('这些 Agent 安装后可以从通用 Skill 目录读取此 Skill。仅在还需要于其 Skill 目录中创建链接或副本时选择。');
    expect(zhCN.agentSelection.ownDirectory.manage.description)
      .toBe('这些 Agent 已可从通用 Skill 目录读取此 Skill。选中后，它们会在自己的 Skill 目录中保留链接或副本。');
    expect(zhCN.agentSelection.ownDirectory.copyToProject.description)
      .toBe('复制完成后，这些 Agent 可以从目标 Project 的通用 Skill 目录读取此 Skill。仅在还需要于其 Skill 目录中创建链接或副本时选择。');
    expect(zhCN.agentSelection.ownDirectory.selectedCount)
      .toBe('已选择 {{count}} 个 Agent');
    expect(en.agentSelection.ownDirectory.title)
      .toBe('Also install in each Agent’s own Skill directory (optional)');
  });
});
