import type {
  AgentSelectionItemId,
  AgentSelectionSnapshot,
  InstallMode,
  ManageSelectionItemState,
} from '@/bindings';

export interface AgentSelectionSession {
  knownItemIds: AgentSelectionItemId[];
  initialSelectedItemIds: AgentSelectionItemId[];
  selectedItemIds: AgentSelectionItemId[];
  mode: InstallMode;
  initialMode: InstallMode;
  otherAgentsExpanded: boolean;
  additionalInstallExpanded: boolean;
  expandedGroupIds: string[];
  requiresReconfirmation: boolean;
}

export function createAgentSelectionSession(
  snapshot: AgentSelectionSnapshot,
  mode: InstallMode = 'symlink',
  itemStates: ManageSelectionItemState[] = [],
): AgentSelectionSession {
  const selected = uniqueSelectable(snapshot, snapshot.initialSelectedItemIds);
  const selectedSet = new Set(selected);
  const agentById = new Map(snapshot.agents.map((agent) => [agent.id, agent]));
  const stateById = new Map(itemStates.map((state) => [state.itemId, state]));
  const isVisibleByDefault = (itemId: AgentSelectionItemId) => {
    const item = snapshot.items.find((candidate) => candidate.id === itemId);
    const state = stateById.get(itemId);
    return selectedSet.has(itemId)
      || item?.disabledReason !== null
      || (state !== undefined && (state.currentEntry !== 'none' || state.disabledReason !== null));
  };
  const hasHiddenSelection = snapshot.items.some((item) => (
    isVisibleByDefault(item.id)
    && item.agentIds.some((id) => agentById.get(id)?.detection !== 'detected')
  ));
  const additionalInstallExpanded = snapshot.items.some((item) => (
    item.category === 'additionalInstall' && isVisibleByDefault(item.id)
  ));
  const expandedGroupIds = snapshot.groups
    .filter((group) => group.itemIds.some(isVisibleByDefault))
    .map((group) => group.id);

  return {
    knownItemIds: snapshot.items.map((item) => item.id),
    initialSelectedItemIds: selected,
    selectedItemIds: selected,
    mode,
    initialMode: mode,
    otherAgentsExpanded: hasHiddenSelection,
    additionalInstallExpanded,
    expandedGroupIds,
    requiresReconfirmation: false,
  };
}

export function toggleSelectionItem(
  session: AgentSelectionSession,
  snapshot: AgentSelectionSnapshot,
  itemId: AgentSelectionItemId,
  selected: boolean,
): AgentSelectionSession {
  const item = snapshot.items.find((candidate) => candidate.id === itemId);
  if (!item?.selectable) return session;
  const next = new Set(session.selectedItemIds);
  if (selected) next.add(itemId);
  else next.delete(itemId);
  return { ...session, selectedItemIds: [...next] };
}

export function toggleSelectionGroup(
  session: AgentSelectionSession,
  snapshot: AgentSelectionSnapshot,
  groupId: string,
  selected: boolean,
): AgentSelectionSession {
  const group = snapshot.groups.find((candidate) => candidate.id === groupId);
  if (!group) return session;
  return group.itemIds.reduce(
    (next, itemId) => toggleSelectionItem(next, snapshot, itemId, selected),
    session,
  );
}

export function groupSelectionState(
  session: AgentSelectionSession,
  snapshot: AgentSelectionSnapshot,
  groupId: string,
): boolean | 'indeterminate' {
  const group = snapshot.groups.find((candidate) => candidate.id === groupId);
  const selectable = group?.itemIds.filter((itemId) => (
    snapshot.items.some((item) => item.id === itemId && item.selectable)
  )) ?? [];
  const selected = new Set(session.selectedItemIds);
  const selectedCount = selectable.filter((itemId) => selected.has(itemId)).length;
  if (selectedCount === 0) return false;
  if (selectedCount === selectable.length) return true;
  return 'indeterminate';
}

export function refreshAgentSelectionSession(
  session: AgentSelectionSession,
  snapshot: AgentSelectionSnapshot,
  itemStates: ManageSelectionItemState[] = [],
): AgentSelectionSession {
  const currentIds = new Set(snapshot.items.map((item) => item.id));
  const knownIds = new Set(session.knownItemIds);
  const retained = session.selectedItemIds.filter((id) => currentIds.has(id));
  const retainedSet = new Set(retained);
  const selectedItemIds = [
    ...retained,
    ...snapshot.initialSelectedItemIds.filter((id) => (
      !knownIds.has(id) && !retainedSet.has(id)
    )),
  ];
  const groupIds = new Set(snapshot.groups.map((group) => group.id));
  const defaults = createAgentSelectionSession(snapshot, session.mode, itemStates);
  return {
    ...session,
    knownItemIds: snapshot.items.map((item) => item.id),
    initialSelectedItemIds: uniqueSelectable(snapshot, snapshot.initialSelectedItemIds),
    selectedItemIds: uniqueSelectable(snapshot, selectedItemIds),
    otherAgentsExpanded: session.otherAgentsExpanded || defaults.otherAgentsExpanded,
    additionalInstallExpanded: session.additionalInstallExpanded
      || defaults.additionalInstallExpanded,
    expandedGroupIds: [...new Set([
      ...session.expandedGroupIds.filter((id) => groupIds.has(id)),
      ...defaults.expandedGroupIds,
    ])],
    requiresReconfirmation: true,
  };
}

export function hasUserSelectionChanges(session: AgentSelectionSession): boolean {
  return !sameSet(session.selectedItemIds, session.initialSelectedItemIds)
    || session.mode !== session.initialMode;
}

export function shouldShowInstallMode(snapshot: AgentSelectionSnapshot): boolean {
  return snapshot.requestedModeItemIds.length > 0;
}

export function isInstallModeDisabled(
  session: AgentSelectionSession,
  snapshot: AgentSelectionSnapshot,
): boolean {
  const requested = new Set(snapshot.requestedModeItemIds);
  return !session.selectedItemIds.some((id) => requested.has(id));
}

function uniqueSelectable(
  snapshot: AgentSelectionSnapshot,
  ids: AgentSelectionItemId[],
): AgentSelectionItemId[] {
  const selectable = new Set(
    snapshot.items.filter((item) => item.selectable).map((item) => item.id),
  );
  return [...new Set(ids)].filter((id) => selectable.has(id));
}

function sameSet(left: string[], right: string[]): boolean {
  if (left.length !== right.length) return false;
  const values = new Set(left);
  return right.every((value) => values.has(value));
}
