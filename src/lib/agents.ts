import type {
  AgentId,
  AgentRuntimeSnapshot,
  ResolvedAgent,
  InstalledSkillLocation,
} from '@/bindings';

export function agentId(agent: ResolvedAgent): AgentId {
  return agent.definition.id;
}

export function agentDisplayName(agent: ResolvedAgent): string {
  return agent.definition.displayName;
}

export function agentsForScope(
  snapshot: AgentRuntimeSnapshot,
  scope: InstalledSkillLocation,
): ResolvedAgent[] {
  return Object.values(snapshot.agents)
    .filter((agent): agent is ResolvedAgent => agent !== undefined)
    .filter((agent) => agent[scope].enabled);
}

export function isAgentSelectable(agent: ResolvedAgent): boolean {
  return agent.global.enabled || agent.project.enabled;
}

export function isAgentDetected(agent: ResolvedAgent): boolean {
  return agent.detection === 'detected';
}
