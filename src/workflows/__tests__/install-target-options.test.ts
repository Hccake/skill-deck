import { describe, expect, it } from 'vitest';
import { makeResolvedAgent, makeResolvedAgentScope } from '@/test-utils';
import {
  initializeInstallTargetSelection,
  reconcileInstallTargetSelection,
} from '../install-target-options';

describe('install target options', () => {
  it('uses the current Context defaults when target selection is initialized', () => {
    const privateAgent = makeResolvedAgent({
      id: 'private-agent',
      displayName: 'Private Agent',
      global: makeResolvedAgentScope({
        readsShared: false,
        privatePath: '~/.private-agent/skills',
        readPaths: ['~/.private-agent/skills'],
      }),
    });

    expect(initializeInstallTargetSelection({
      scope: 'global',
      preselectedAgents: [],
      mode: 'symlink',
      facts: {
        allAgents: [privateAgent],
        selectionGroups: [],
        availableAgentTargets: [],
        defaultAgents: ['private-agent'],
        defaultsUnavailable: false,
      },
    })).toEqual({
      selectedAgents: ['private-agent'],
      privateCopyAgents: [],
      selectedAgentTargets: [],
      mode: 'symlink',
    });
  });

  it('prefers CLI targets and initializes the concrete targets owned by them', () => {
    const eve = makeResolvedAgent({
      id: 'eve',
      project: makeResolvedAgentScope({
        readsShared: false,
        privatePath: './agent/skills',
        readPaths: ['./agent/skills'],
      }),
    });

    expect(initializeInstallTargetSelection({
      scope: 'project',
      preselectedAgents: ['eve'],
      mode: 'copy',
      facts: {
        allAgents: [eve],
        selectionGroups: [],
        availableAgentTargets: [{
          targetId: 'eve:research',
          agent: 'eve',
          displayName: 'Eve (research)',
          subagent: 'research',
          path: '/project/agent/subagents/research/skills',
        }],
        defaultAgents: [],
        defaultsUnavailable: false,
      },
    })).toEqual({
      selectedAgents: ['eve'],
      privateCopyAgents: [],
      selectedAgentTargets: [{ agentId: 'eve', targetId: 'eve:research' }],
      mode: 'copy',
    });
  });

  it('uses detected Built-in recommendations only when saved defaults are absent', () => {
    const claude = makeResolvedAgent({
      id: 'claude-code',
      detection: 'detected',
      global: makeResolvedAgentScope({
        readsShared: false,
        privatePath: '~/.claude/skills',
      }),
    });
    const cursor = makeResolvedAgent({
      id: 'cursor',
      detection: 'notDetected',
      global: makeResolvedAgentScope({
        readsShared: false,
        privatePath: '~/.cursor/skills',
      }),
    });

    expect(initializeInstallTargetSelection({
      scope: 'global',
      preselectedAgents: [],
      mode: 'symlink',
      facts: {
        allAgents: [claude, cursor],
        selectionGroups: [],
        availableAgentTargets: [],
        defaultAgents: null,
        defaultsUnavailable: true,
      },
    }).selectedAgents).toEqual(['claude-code']);
  });

  it('preserves valid explicit targets while removing targets absent from refreshed facts', () => {
    const privateAgent = makeResolvedAgent({
      id: 'private-agent',
      project: makeResolvedAgentScope({
        readsShared: false,
        privatePath: './.private-agent/skills',
        readPaths: ['./.private-agent/skills'],
      }),
    });
    const sharedAgent = makeResolvedAgent({
      id: 'shared-agent',
      project: makeResolvedAgentScope({
        readsShared: true,
        privatePath: './.shared-agent/skills',
        readPaths: ['./.agents/skills', './.shared-agent/skills'],
      }),
    });
    const eve = makeResolvedAgent({
      id: 'eve',
      project: makeResolvedAgentScope({
        readsShared: false,
        privatePath: './agent/skills',
        readPaths: ['./agent/skills'],
      }),
    });

    expect(reconcileInstallTargetSelection({
      scope: 'project',
      selection: {
        selectedAgents: ['private-agent', 'eve', 'removed-agent'],
        privateCopyAgents: ['shared-agent', 'removed-agent'],
        selectedAgentTargets: [
          { agentId: 'eve', targetId: 'eve:research' },
          { agentId: 'eve', targetId: 'eve:removed' },
        ],
        mode: 'copy',
      },
      facts: {
        allAgents: [privateAgent, sharedAgent, eve],
        selectionGroups: [],
        availableAgentTargets: [{
          targetId: 'eve:research',
          agent: 'eve',
          displayName: 'Eve (research)',
          subagent: 'research',
          path: '/project/agent/subagents/research/skills',
        }],
        defaultAgents: null,
        defaultsUnavailable: false,
      },
    })).toEqual({
      selectedAgents: ['private-agent', 'eve'],
      privateCopyAgents: ['shared-agent'],
      selectedAgentTargets: [{ agentId: 'eve', targetId: 'eve:research' }],
      mode: 'copy',
    });
  });
});
