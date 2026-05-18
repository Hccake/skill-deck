import type { AgentInfo, DefaultTargetAgents } from '@/bindings';

export type InstallScope = 'global' | 'project';

export const EMPTY_DEFAULT_TARGET_AGENTS: DefaultTargetAgents = {
  global: [],
  project: [],
};

export function getAgentTarget(agent: AgentInfo, scope: InstallScope) {
  return agent.targets[scope];
}

export function isAutomaticAgent(agent: AgentInfo, scope: InstallScope): boolean {
  const target = getAgentTarget(agent, scope);
  return target.supported && target.automatic;
}

export function isAdditionalAgent(agent: AgentInfo, scope: InstallScope): boolean {
  const target = getAgentTarget(agent, scope);
  return target.supported && !target.automatic;
}

export function filterAdditionalAgentIds(
  ids: string[],
  agents: AgentInfo[],
  scope: InstallScope,
): string[] {
  const agentById = new Map<string, AgentInfo>(
    agents.map((agent) => [agent.id, agent])
  );
  const result: string[] = [];

  for (const id of ids) {
    const agent = agentById.get(id);
    if (!agent) continue;

    if (!isAdditionalAgent(agent, scope)) continue;
    if (!result.includes(id)) result.push(id);
  }

  return result;
}

export function migrateDefaultTargetAgents(
  lastSelectedAgents: string[],
  agents: AgentInfo[],
): DefaultTargetAgents {
  return {
    global: filterAdditionalAgentIds(lastSelectedAgents, agents, 'global'),
    project: filterAdditionalAgentIds(lastSelectedAgents, agents, 'project'),
  };
}
