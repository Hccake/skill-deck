import type {
  AgentInstallOptionId,
  AgentSelectionSnapshot,
  InstallMode,
  ManageInstallOptionState,
} from '@/bindings';
import { projectAgentSelectionView } from '@/lib/agent-selection-view';

export interface AgentSelectionSession {
  knownOptionIds: AgentInstallOptionId[];
  initialSelectedOptionIds: AgentInstallOptionId[];
  selectedOptionIds: AgentInstallOptionId[];
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
  optionStates: ManageInstallOptionState[] = [],
): AgentSelectionSession {
  const selected = uniqueSelectable(snapshot, snapshot.initialSelectedOptionIds);
  const selectedSet = new Set(selected);
  const { agentsById, additionalOptions } = projectAgentSelectionView(snapshot);
  const stateById = new Map(optionStates.map((state) => [state.optionId, state]));
  const isVisibleByDefault = (optionId: AgentInstallOptionId) => {
    const option = snapshot.installOptions.find((candidate) => candidate.id === optionId);
    const state = stateById.get(optionId);
    return selectedSet.has(optionId)
      || option?.disabledReason !== null
      || (state !== undefined && (state.currentEntry !== 'none' || state.disabledReason !== null));
  };
  const hasHiddenSelection = snapshot.installOptions.some((option) => (
    option.kind === 'standardDirectory'
    && isVisibleByDefault(option.id)
    && option.agentIds.some((id) => agentsById.get(id)?.detection !== 'detected')
  ));
  const additionalInstallExpanded = additionalOptions.some((option) => (
    isVisibleByDefault(option.id)
  ));
  const expandedGroupIds = snapshot.groups
    .filter((group) => group.optionIds.some(isVisibleByDefault))
    .map((group) => group.id);

  return {
    knownOptionIds: snapshot.installOptions.map((option) => option.id),
    initialSelectedOptionIds: selected,
    selectedOptionIds: selected,
    mode,
    initialMode: mode,
    otherAgentsExpanded: hasHiddenSelection,
    additionalInstallExpanded,
    expandedGroupIds,
    requiresReconfirmation: false,
  };
}

export function toggleInstallOption(
  session: AgentSelectionSession,
  snapshot: AgentSelectionSnapshot,
  optionId: AgentInstallOptionId,
  selected: boolean,
): AgentSelectionSession {
  const option = snapshot.installOptions.find((candidate) => candidate.id === optionId);
  if (!option?.selectable) return session;
  const next = new Set(session.selectedOptionIds);
  if (selected) next.add(optionId);
  else next.delete(optionId);
  return { ...session, selectedOptionIds: [...next] };
}

export function toggleSelectionGroup(
  session: AgentSelectionSession,
  snapshot: AgentSelectionSnapshot,
  groupId: string,
  selected: boolean,
): AgentSelectionSession {
  const group = snapshot.groups.find((candidate) => candidate.id === groupId);
  if (!group) return session;
  return group.optionIds.reduce(
    (next, optionId) => toggleInstallOption(next, snapshot, optionId, selected),
    session,
  );
}

export function groupSelectionState(
  session: AgentSelectionSession,
  snapshot: AgentSelectionSnapshot,
  groupId: string,
): boolean | 'indeterminate' {
  const group = snapshot.groups.find((candidate) => candidate.id === groupId);
  const selectable = group?.optionIds.filter((optionId) => (
    snapshot.installOptions.some((option) => option.id === optionId && option.selectable)
  )) ?? [];
  const selected = new Set(session.selectedOptionIds);
  const selectedCount = selectable.filter((optionId) => selected.has(optionId)).length;
  if (selectedCount === 0) return false;
  if (selectedCount === selectable.length) return true;
  return 'indeterminate';
}

export function refreshAgentSelectionSession(
  session: AgentSelectionSession,
  snapshot: AgentSelectionSnapshot,
  optionStates: ManageInstallOptionState[] = [],
): AgentSelectionSession {
  const currentIds = new Set(snapshot.installOptions.map((option) => option.id));
  const knownIds = new Set(session.knownOptionIds);
  const retained = session.selectedOptionIds.filter((id) => currentIds.has(id));
  const retainedSet = new Set(retained);
  const selectedOptionIds = [
    ...retained,
    ...snapshot.initialSelectedOptionIds.filter((id) => (
      !knownIds.has(id) && !retainedSet.has(id)
    )),
  ];
  const groupIds = new Set(snapshot.groups.map((group) => group.id));
  const defaults = createAgentSelectionSession(snapshot, session.mode, optionStates);
  return {
    ...session,
    knownOptionIds: snapshot.installOptions.map((option) => option.id),
    initialSelectedOptionIds: uniqueSelectable(snapshot, snapshot.initialSelectedOptionIds),
    selectedOptionIds: uniqueSelectable(snapshot, selectedOptionIds),
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

export function hasUserSelectionChanges(
  session: AgentSelectionSession,
  snapshot: AgentSelectionSnapshot,
): boolean {
  if (!sameSet(session.selectedOptionIds, session.initialSelectedOptionIds)) return true;
  if (session.mode === session.initialMode) return false;
  const modeOptionIds = new Set(snapshot.userModeOptionIds);
  return session.selectedOptionIds.some((id) => modeOptionIds.has(id));
}

export function shouldShowInstallMode(snapshot: AgentSelectionSnapshot): boolean {
  return snapshot.userModeOptionIds.length > 0;
}

export function preserveOwnDirectoryOptions(
  snapshot: AgentSelectionSnapshot,
  agentIds: string[],
): AgentInstallOptionId[] {
  const requested = new Set(agentIds);
  const selected = new Set(snapshot.initialSelectedOptionIds);
  for (const option of snapshot.installOptions) {
    if (
      option.kind === 'standardDirectory'
      && option.selectable
      && option.agentIds.some((agentId) => requested.has(agentId))
    ) {
      selected.add(option.id);
    }
  }
  return [...selected];
}

function uniqueSelectable(
  snapshot: AgentSelectionSnapshot,
  ids: AgentInstallOptionId[],
): AgentInstallOptionId[] {
  const selectable = new Set(
    snapshot.installOptions.filter((option) => option.selectable).map((option) => option.id),
  );
  return [...new Set(ids)].filter((id) => selectable.has(id));
}

function sameSet(left: string[], right: string[]): boolean {
  if (left.length !== right.length) return false;
  const values = new Set(left);
  return right.every((value) => values.has(value));
}
