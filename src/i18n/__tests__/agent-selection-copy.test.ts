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

  it('describes install history as a confirmed choice instead of an install result', () => {
    expect(zhCN.addSkill.agents.historyLoadWarning)
      .toBe('未能读取最近确认的 Agent 选择。本次安装仍可继续选择目标。');
    expect(zhCN.addSkill.agents.historySaveWarning)
      .toBe('可以继续安装，但未能保存本次确认的 Agent 选择。');
    expect(en.addSkill.agents.historyLoadWarning)
      .toBe('Recent confirmed Agent choices could not be loaded. You can still choose targets for this installation.');
    expect(en.addSkill.agents.historySaveWarning)
      .toBe('The installation can continue, but the confirmed Agent choices could not be saved.');
  });

  it('distinguishes Library availability from a direct Agent association', () => {
    expect(zhCN.agentSelection.current.library).toBe('通过 Skill 库可用');
    expect(zhCN.agentSelection.effect.restoreLibrary).toBe('将改用 Skill 库版本');
    expect(en.agentSelection.current.library).toBe('Available from a Skill Library');
    expect(en.agentSelection.effect.restoreLibrary).toBe('Will use the Skill Library version');
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
    expect(zhCN.skills.copyToProject.agentSelectionDescription)
      .toBe('Agent 会按照源 Skill 的关联状态预先勾选；目标项目已有的关联会保留。');
    expect(en.skills.copyToProject.agentSelectionDescription)
      .toBe('Agents are preselected from the source Skill. Existing target-project associations are preserved.');
  });

  it('presents own-directory installations as an optional nested setting', () => {
    expect(zhCN.agentSelection.ownDirectory.title)
      .toBe('同时安装到 Agent 自己的 Skill 目录（可选）');
    for (const usage of ['install', 'manage', 'copyToProject', 'libraryApplication']) {
      expect(usage in zhCN.agentSelection.ownDirectory).toBe(false);
      expect(usage in en.agentSelection.ownDirectory).toBe(false);
    }
    expect(zhCN.agentSelection.ownDirectory.selectedCount)
      .toBe('已选择 {{count}} 个 Agent');
    expect(en.agentSelection.ownDirectory.title)
      .toBe('Also install in each Agent’s own Skill directory (optional)');
  });
});
