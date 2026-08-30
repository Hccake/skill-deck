import { describe, expect, it } from 'vitest';
import type { AgentSelectionSnapshot, ManageInstallOptionState } from '@/bindings';
import {
  createAgentSelectionSession,
  groupSelectionState,
  hasUserSelectionChanges,
  refreshAgentSelectionSession,
  toggleSelectionGroup,
  toggleInstallOption,
} from '../agent-selection-session';

function snapshot(): AgentSelectionSnapshot {
  return {
    agents: [{ kind: 'grouped', id: 'eve', displayName: 'Eve', detection: 'detected', directoryAccess: null, installOptionId: null, groupId: 'eve' }],
    installOptions: [
      { id: 'root', kind: 'groupLocation', agentIds: ['eve'], displayName: '根 Agent', path: '/root', groupId: 'eve', selectable: true, modeConstraint: 'copyOnly', disabledReason: null },
      { id: 'writer', kind: 'groupLocation', agentIds: ['eve'], displayName: 'Writer', path: '/writer', groupId: 'eve', selectable: true, modeConstraint: 'copyOnly', disabledReason: null },
    ],
    groups: [{ id: 'eve', agentId: 'eve', displayName: 'Eve', optionIds: ['root', 'writer'], detection: 'detected' }],
    initialSelectedOptionIds: ['root'],
    unavailableExplicitAgents: [],
    userModeOptionIds: [],
    revision: 'revision-1',
  };
}

describe('Agent selection session', () => {
  it('keeps parent selection independent from expansion state', () => {
    const current = createAgentSelectionSession(snapshot());
    expect(groupSelectionState(current, snapshot(), 'eve')).toBe('indeterminate');

    const selected = toggleSelectionGroup(current, snapshot(), 'eve', true);
    expect(groupSelectionState(selected, snapshot(), 'eve')).toBe(true);
    expect(selected.expandedGroupIds).toEqual(['eve']);
  });

  it('retains only stable option ids when a snapshot is refreshed', () => {
    const current = toggleSelectionGroup(
      createAgentSelectionSession(snapshot()),
      snapshot(),
      'eve',
      true,
    );
    const next = snapshot();
    next.installOptions = next.installOptions.filter((option) => option.id === 'root');
    next.groups[0].optionIds = ['root'];
    next.revision = 'revision-2';

    const refreshed = refreshAgentSelectionSession(current, next);
    expect(refreshed.selectedOptionIds).toEqual(['root']);
    expect(refreshed.requiresReconfirmation).toBe(true);
  });

  it('keeps an explicit deselection and applies defaults only to newly discovered options', () => {
    const current = toggleInstallOption(
      createAgentSelectionSession(snapshot()),
      snapshot(),
      'root',
      false,
    );
    const next = snapshot();
    next.installOptions.push({ id: 'new-target', kind: 'groupLocation', agentIds: ['eve'], displayName: '新目录', path: '/new', groupId: 'eve', selectable: true, modeConstraint: 'copyOnly', disabledReason: null });
    next.groups[0].optionIds.push('new-target');
    next.initialSelectedOptionIds = ['root', 'new-target'];
    next.revision = 'revision-2';

    const refreshed = refreshAgentSelectionSession(current, next);

    expect(refreshed.selectedOptionIds).toEqual(['new-target']);
    expect(refreshed.knownOptionIds).toEqual(['root', 'writer', 'new-target']);
  });

  it('expands a group that contains an existing or abnormal directory entry', () => {
    const currentSnapshot = snapshot();
    currentSnapshot.initialSelectedOptionIds = [];
    const states: ManageInstallOptionState[] = [{
      optionId: 'writer',
      currentEntry: 'unrecognized',
      currentVersion: 'external',
      initialSelected: false,
      allowedResults: 'none',
      selectedEffect: null,
      unselectedEffect: null,
      disabledReason: 'unrecognizedEntry',
    }];

    expect(createAgentSelectionSession(currentSnapshot, 'symlink', states).expandedGroupIds)
      .toEqual(['eve']);
  });

  it('counts an installation-mode choice only when it applies to a selected option', () => {
    const currentSnapshot = snapshot();
    currentSnapshot.agents = [{ kind: 'standard', id: 'cursor', displayName: 'Cursor', detection: 'detected', directoryAccess: 'privateOnly', installOptionId: 'cursor', groupId: null }];
    currentSnapshot.installOptions = [{
      id: 'cursor',
      kind: 'standardDirectory',
      agentIds: ['cursor'],
      displayName: 'Cursor',
      path: '/cursor',
      groupId: null,
      selectable: true,
      modeConstraint: 'userSelectable',
      disabledReason: null,
    }];
    currentSnapshot.groups = [];
    currentSnapshot.userModeOptionIds = ['cursor'];
    currentSnapshot.initialSelectedOptionIds = [];
    const inactiveChoice = {
      ...createAgentSelectionSession(currentSnapshot),
      mode: 'copy' as const,
    };

    expect(hasUserSelectionChanges(inactiveChoice, currentSnapshot)).toBe(false);

    currentSnapshot.initialSelectedOptionIds = ['cursor'];
    const activeChoice = {
      ...createAgentSelectionSession(currentSnapshot),
      mode: 'copy' as const,
    };
    expect(hasUserSelectionChanges(activeChoice, currentSnapshot)).toBe(true);
  });
});
