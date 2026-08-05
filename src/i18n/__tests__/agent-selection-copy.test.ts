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
});
