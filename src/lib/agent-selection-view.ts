import type {
  AgentInstallOption,
  AgentSelectionAgent,
  AgentSelectionSnapshot,
} from '@/bindings';

export interface AgentSelectionViewProjection {
  agentsById: Map<string, AgentSelectionAgent>;
  directAgents: AgentSelectionAgent[];
  separateOptions: AgentInstallOption[];
  additionalOptions: AgentInstallOption[];
}

export function projectAgentSelectionView(
  snapshot: AgentSelectionSnapshot,
): AgentSelectionViewProjection {
  const agentsById = new Map(snapshot.agents.map((agent) => [agent.id, agent]));
  const directAgents = snapshot.agents.filter((agent) => (
    agent.directoryAccess === 'standardOnly' || agent.directoryAccess === 'both'
  ));
  const standardOptions = snapshot.installOptions.filter((option) => (
    option.kind === 'standardDirectory' && option.groupId === null
  ));
  const separateOptions = standardOptions.filter((option) => option.agentIds.some((agentId) => (
    agentsById.get(agentId)?.directoryAccess === 'privateOnly'
  )));
  const separateIds = new Set(separateOptions.map((option) => option.id));
  const additionalOptions = standardOptions.filter((option) => !separateIds.has(option.id));

  return {
    agentsById,
    directAgents,
    separateOptions,
    additionalOptions,
  };
}
