import type { AgentId, AgentSelectionGroup, ResolvedAgent } from '@/bindings';
import { agentId } from './agents';

export interface AgentSelectionRow {
  groupId: string;
  agents: ResolvedAgent[];
  selectableAgentIds: AgentId[];
}

export function buildAgentSelectionRows(
  allAgents: ResolvedAgent[],
  backendGroups: AgentSelectionGroup[],
  selectableAgents: ResolvedAgent[],
): AgentSelectionRow[] {
  const agentById = new Map(allAgents.map((agent) => [agentId(agent), agent]));
  const orderById = new Map(allAgents.map((agent, index) => [agentId(agent), index]));
  const selectableIds = new Set(selectableAgents.map(agentId));
  const groupedSelectableIds = new Set<AgentId>();
  const rows: Array<AgentSelectionRow & { order: number }> = [];

  for (const group of backendGroups) {
    const groupAgents = group.agentIds.flatMap((id) => {
      const agent = agentById.get(id);
      return agent ? [agent] : [];
    });
    const groupSelectableIds = group.agentIds.filter((id) => selectableIds.has(id));
    if (groupSelectableIds.length === 0) continue;

    for (const id of groupSelectableIds) groupedSelectableIds.add(id);
    rows.push({
      groupId: group.groupId,
      agents: groupAgents,
      selectableAgentIds: groupSelectableIds,
      order: Math.min(...groupAgents.map((agent) => orderById.get(agentId(agent)) ?? Number.MAX_SAFE_INTEGER)),
    });
  }

  for (const agent of selectableAgents) {
    const id = agentId(agent);
    if (groupedSelectableIds.has(id)) continue;
    rows.push({
      groupId: `agent:${id}`,
      agents: [agent],
      selectableAgentIds: [id],
      order: orderById.get(id) ?? Number.MAX_SAFE_INTEGER,
    });
  }

  return rows
    .sort((left, right) => left.order - right.order)
    .map(({ order: _order, ...row }) => row);
}

export function isSelectionRowSelected(
  row: AgentSelectionRow,
  selectedAgentIds: ReadonlySet<AgentId>,
): boolean {
  return row.selectableAgentIds.some((id) => selectedAgentIds.has(id));
}

export function toggleSelectionRow(
  current: AgentId[],
  rowAgentIds: AgentId[],
): AgentId[] {
  const rowIds = new Set(rowAgentIds);
  const selected = current.some((id) => rowIds.has(id));
  if (selected) return current.filter((id) => !rowIds.has(id));

  const next = [...current];
  for (const id of rowAgentIds) {
    if (!next.includes(id)) next.push(id);
  }
  return next;
}
