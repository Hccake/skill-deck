import type {
  AgentId,
  AgentWriteIntent,
  InstallResponse,
  ResolvedAgent,
} from '@/bindings';
import { agentId } from './agents';
import { getAgentTarget, type InstallScope } from './agentTargets';

export interface AdapterTargetSelection {
  agentId: AgentId;
  targetId: string;
}

interface BuildAgentWriteIntentsInput {
  agents: ResolvedAgent[];
  scope: InstallScope;
  selectedAgents: AgentId[];
  privateCopyAgents: AgentId[];
  adapterTargets: AdapterTargetSelection[];
}

export function buildAgentWriteIntents({
  agents,
  scope,
  selectedAgents,
  privateCopyAgents,
  adapterTargets,
}: BuildAgentWriteIntentsInput): AgentWriteIntent[] {
  const selected = new Set(selectedAgents);
  const privateCopies = new Set(privateCopyAgents);
  const targetsByAgent = new Map<AgentId, string[]>();

  for (const target of adapterTargets) {
    const targets = targetsByAgent.get(target.agentId) ?? [];
    if (!targets.includes(target.targetId)) targets.push(target.targetId);
    targetsByAgent.set(target.agentId, targets);
  }

  const intents: AgentWriteIntent[] = [];
  for (const agent of agents) {
    const id = agentId(agent);
    const target = getAgentTarget(agent, scope);
    const adapterTargetIds = targetsByAgent.get(id) ?? [];
    const privateEntry = selected.has(id) && !target.readsShared
      ? 'required'
      : privateCopies.has(id)
        ? 'optionalSelected'
        : 'none';

    if (privateEntry !== 'none' || adapterTargetIds.length > 0) {
      intents.push({
        agentId: id,
        privateEntry,
        adapterTargets: [...adapterTargetIds].sort(),
      });
    }
    targetsByAgent.delete(id);
  }

  for (const [id, adapterTargetIds] of targetsByAgent) {
    intents.push({
      agentId: id,
      privateEntry: 'none',
      adapterTargets: [...adapterTargetIds].sort(),
    });
  }

  return intents.sort((left, right) => left.agentId.localeCompare(right.agentId));
}

export function hasFailedMutationUnits(response: InstallResponse): boolean {
  return response.units.some((unit) => unit.status !== 'succeeded');
}
