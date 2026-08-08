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
      installOptionId: 'standard-private',
      groupId: null,
    },
    {
      kind: 'standard',
      id: 'cursor',
      displayName: 'Cursor',
      detection: 'detected',
      directoryAccess: 'both',
      installOptionId: 'standard-private',
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
      id: 'standard-private',
      kind: 'standardDirectory',
      agentIds: ['claude-code', 'cursor'],
      displayName: 'Claude Code',
      path: '/standard/private',
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
  userModeOptionIds: ['standard-private', 'codex-private'],
  revision: 'selection-v2',
};

describe('Agent selection view projection', () => {
  it('shows a mixed standard directory once while preserving direct-use Agents', () => {
    const projected = projectAgentSelectionView(snapshot);

    expect(projected.directAgents.map((agent) => agent.id)).toEqual(['cursor', 'codex']);
    expect(projected.separateOptions.map((option) => option.id)).toEqual(['standard-private']);
    expect(projected.additionalOptions.map((option) => option.id)).toEqual(['codex-private']);
  });
});
