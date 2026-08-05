import { describe, expect, it } from 'vitest';
import type { AgentSelectionSnapshot } from '@/bindings';
import { projectAgentSelectionView } from '../agent-selection-view';

const snapshot: AgentSelectionSnapshot = {
  agents: [
    {
      kind: 'standard',
      id: 'claude-code',
      displayName: 'Claude Code',
      detection: 'detected',
      directoryAccess: 'privateOnly',
      installOptionId: 'shared-private',
      groupId: null,
    },
    {
      kind: 'standard',
      id: 'cursor',
      displayName: 'Cursor',
      detection: 'detected',
      directoryAccess: 'both',
      installOptionId: 'shared-private',
      groupId: null,
    },
    {
      kind: 'standard',
      id: 'codex',
      displayName: 'Codex',
      detection: 'notDetected',
      directoryAccess: 'both',
      installOptionId: 'codex-private',
      groupId: null,
    },
  ],
  installOptions: [
    {
      id: 'shared-private',
      kind: 'standardDirectory',
      agentIds: ['claude-code', 'cursor'],
      displayName: 'Claude Code',
      path: '/shared/private',
      groupId: null,
      selectable: true,
      modeConstraint: 'userSelectable',
      disabledReason: null,
    },
    {
      id: 'codex-private',
      kind: 'standardDirectory',
      agentIds: ['codex'],
      displayName: 'Codex',
      path: '/codex/private',
      groupId: null,
      selectable: true,
      modeConstraint: 'userSelectable',
      disabledReason: null,
    },
  ],
  groups: [],
  initialSelectedOptionIds: [],
  unavailableExplicitAgents: [],
  userModeOptionIds: ['shared-private', 'codex-private'],
  revision: 'selection-v2',
};

describe('Agent selection view projection', () => {
  it('shows a mixed shared directory once while preserving direct-use Agents', () => {
    const projected = projectAgentSelectionView(snapshot);

    expect(projected.directAgents.map((agent) => agent.id)).toEqual(['cursor', 'codex']);
    expect(projected.separateOptions.map((option) => option.id)).toEqual(['shared-private']);
    expect(projected.additionalOptions.map((option) => option.id)).toEqual(['codex-private']);
  });
});
