import { describe, expect, it } from 'vitest';
import en from '../locales/en.json';
import zhCN from '../locales/zh-CN.json';

describe('Agent selection copy', () => {
  it('keeps installation-mode wording in the Agent selection namespace', () => {
    expect(zhCN.agentSelection.linkRecommended).toBe('链接（推荐）');
    expect(zhCN.agentSelection.copyOnly).toBe('仅支持复制');
    expect(en.agentSelection.linkRecommended).toBe('Link (recommended)');
    expect(en.agentSelection.copyOnly).toBe('Copy only');
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
      .toBe('这些 Agent 也可读取通用 Skill 目录，当前检测状态未确认。');
    expect(zhCN.agentSelection.memberCount).toBe('{{count}} 个 Agent');
    expect(zhCN.agentSelection.viewMembers).toBe('查看成员');
    expect(zhCN.agentSelection.sharedPlacementDescription)
      .toBe('这些 Agent 共用同一个 Skill 存放位置，选择后将统一处理。');
    expect(zhCN.agentSelection.detection.detected).toBe('已检测到');
    expect(zhCN.agentSelection.detectedCount).toBe('{{detected}}/{{total}} 已检测到');
  });

  it('uses scenario-specific headings and guidance without changing the selection model', () => {
    expect(zhCN.agentSelection.automatic.install.title).toBe('安装后可直接使用');
    expect(zhCN.agentSelection.automatic.install.help)
      .toBe('此 Skill 安装到通用 Skill 目录后，这些 Agent 无需单独安装。');
    expect(zhCN.agentSelection.automatic.manage.title).toBe('可通过通用目录使用');
    expect(zhCN.agentSelection.automatic.manage.help)
      .toBe('这些 Agent 会读取当前 Skill 所在的通用 Skill 目录，无需单独安装。');
    expect(zhCN.agentSelection.automatic.copyToProject.title).toBe('复制后可直接使用');
    expect(zhCN.agentSelection.automatic.copyToProject.help)
      .toBe('此 Skill 复制到目标项目后，这些 Agent 无需单独安装。');
    expect(zhCN.agentSelection.selectable.title).toBe('选择后可使用');
    expect(zhCN.agentSelection.selectable.help)
      .toBe('这些 Agent 不读取通用 Skill 目录，需要单独安装后才能使用此 Skill。');
  });

  it('presents own-directory writes as an optional nested setting', () => {
    expect(zhCN.agentSelection.ownDirectory.title)
      .toBe('同时写入 Agent 自己的 Skill 目录（可选）');
    expect(zhCN.agentSelection.ownDirectory.install.description)
      .toBe('这些 Agent 安装后即可使用此 Skill。仅在需要向其 Skill 目录写入链接或副本时选择。');
    expect(zhCN.agentSelection.ownDirectory.manage.description)
      .toBe('这些 Agent 已可通过通用 Skill 目录使用此 Skill。选中的 Agent 会在自己的 Skill 目录中保留链接或副本。');
    expect(zhCN.agentSelection.ownDirectory.copyToProject.description)
      .toBe('这些 Agent 可通过目标项目的通用 Skill 目录使用此 Skill。仅在需要向其 Skill 目录写入链接或副本时选择。');
    expect(zhCN.agentSelection.ownDirectory.selectedCount)
      .toBe('已选择 {{count}} 个 Agent');
    expect(en.agentSelection.ownDirectory.title)
      .toBe('Also write to each Agent’s own Skill directory (optional)');
  });
});
