import { describe, expect, it } from 'vitest';
import type { AgentInfo } from '@/bindings';
import { filterAdditionalAgentIds, migrateDefaultTargetAgents } from '../agentTargets';

function makeAgent(
  id: string,
  globalAutomatic: boolean,
  projectAutomatic: boolean,
  detected = true,
): AgentInfo {
  return {
    id: id as AgentInfo['id'],
    name: id,
    skillsDir: projectAutomatic ? '.agents/skills' : `.${id}/skills`,
    globalSkillsDir: globalAutomatic ? '~/.agents/skills' : `~/.${id}/skills`,
    detected,
    targets: {
      global: {
        supported: true,
        automatic: globalAutomatic,
        path: globalAutomatic ? '~/.agents/skills' : `~/.${id}/skills`,
      },
      project: {
        supported: true,
        automatic: projectAutomatic,
        path: projectAutomatic ? '.agents/skills' : `.${id}/skills`,
      },
    },
  };
}

describe('agent target helpers', () => {
  it('filters automatic agents from additional defaults per scope', () => {
    const agents = [
      makeAgent('antigravity', false, true),
      makeAgent('warp', true, true),
      makeAgent('claude-code', false, false),
    ];

    expect(filterAdditionalAgentIds(['antigravity', 'warp', 'claude-code'], agents, 'global'))
      .toEqual(['antigravity', 'claude-code']);
    expect(filterAdditionalAgentIds(['antigravity', 'warp', 'claude-code'], agents, 'project'))
      .toEqual(['claude-code']);
  });

  it('migrates lastSelectedAgents independently for global and project', () => {
    const agents = [
      makeAgent('antigravity', false, true),
      makeAgent('codex', false, true),
      makeAgent('claude-code', false, false),
    ];

    expect(migrateDefaultTargetAgents(['antigravity', 'codex', 'claude-code'], agents))
      .toEqual({
        global: ['antigravity', 'codex', 'claude-code'],
        project: ['claude-code'],
      });
  });

  it('drops unsupported and unknown agent ids', () => {
    const unsupported = makeAgent('unsupported', false, false);
    unsupported.targets.global.supported = false;

    expect(filterAdditionalAgentIds(['unsupported', 'missing'], [unsupported], 'global'))
      .toEqual([]);
  });
});
