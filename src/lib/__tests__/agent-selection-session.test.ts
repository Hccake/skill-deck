import { describe, expect, it } from 'vitest';
import type { AgentSelectionSnapshot, ManageSelectionItemState } from '@/bindings';
import {
  createAgentSelectionSession,
  groupSelectionState,
  refreshAgentSelectionSession,
  toggleSelectionGroup,
  toggleSelectionItem,
} from '../agent-selection-session';

function snapshot(): AgentSelectionSnapshot {
  return {
    agents: [{ id: 'eve', displayName: 'Eve', detection: 'detected' }],
    directAgentIds: [],
    items: [
      { id: 'root', agentIds: ['eve'], category: 'groupChild', displayName: '根 Agent', path: '/root', groupId: 'eve', selectable: true, modeConstraint: 'copyOnly', disabledReason: null },
      { id: 'writer', agentIds: ['eve'], category: 'groupChild', displayName: 'Writer', path: '/writer', groupId: 'eve', selectable: true, modeConstraint: 'copyOnly', disabledReason: null },
    ],
    groups: [{ id: 'eve', agentId: 'eve', displayName: 'Eve', itemIds: ['root', 'writer'], detection: 'detected' }],
    initialSelectedItemIds: ['root'],
    unavailableExplicitAgents: [],
    requestedModeItemIds: [],
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

  it('retains only stable item ids when a snapshot is refreshed', () => {
    const current = toggleSelectionGroup(
      createAgentSelectionSession(snapshot()),
      snapshot(),
      'eve',
      true,
    );
    const next = snapshot();
    next.items = next.items.filter((item) => item.id === 'root');
    next.groups[0].itemIds = ['root'];
    next.revision = 'revision-2';

    const refreshed = refreshAgentSelectionSession(current, next);
    expect(refreshed.selectedItemIds).toEqual(['root']);
    expect(refreshed.requiresReconfirmation).toBe(true);
  });

  it('keeps an explicit deselection and applies defaults only to newly discovered items', () => {
    const current = toggleSelectionItem(
      createAgentSelectionSession(snapshot()),
      snapshot(),
      'root',
      false,
    );
    const next = snapshot();
    next.items.push({ id: 'new-target', agentIds: ['eve'], category: 'groupChild', displayName: '新目录', path: '/new', groupId: 'eve', selectable: true, modeConstraint: 'copyOnly', disabledReason: null });
    next.groups[0].itemIds.push('new-target');
    next.initialSelectedItemIds = ['root', 'new-target'];
    next.revision = 'revision-2';

    const refreshed = refreshAgentSelectionSession(current, next);

    expect(refreshed.selectedItemIds).toEqual(['new-target']);
    expect(refreshed.knownItemIds).toEqual(['root', 'writer', 'new-target']);
  });

  it('expands a group that contains an existing or abnormal directory entry', () => {
    const currentSnapshot = snapshot();
    currentSnapshot.initialSelectedItemIds = [];
    const states: ManageSelectionItemState[] = [{
      itemId: 'writer',
      currentEntry: 'unrecognized',
      initialSelected: false,
      allowedResults: 'none',
      selectedEffect: null,
      unselectedEffect: null,
      disabledReason: 'unrecognizedEntry',
    }];

    expect(createAgentSelectionSession(currentSnapshot, 'symlink', states).expandedGroupIds)
      .toEqual(['eve']);
  });
});
