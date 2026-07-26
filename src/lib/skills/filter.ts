import type { AgentId, InstalledSkill, ResolvedAgent } from '@/bindings';
import { agentDisplayName, agentId, isAgentDetected } from '@/lib/agents';

/** 返回 Backend 投影出的、当前可用于 Skill card 展示和筛选的 Agent。 */
export function getSkillAssociatedAgentIds(skill: InstalledSkill): AgentId[] {
  return Array.from(new Set(skill.associatedAgents));
}

export function filterSkills<T extends InstalledSkill>(
  skills: T[],
  searchQuery: string,
  agentFilter: AgentId | null,
): T[] {
  const query = searchQuery.trim().toLowerCase();
  if (!query && agentFilter === null) return skills;

  return skills.filter((skill) => {
    if (
      query
      && !skill.name.toLowerCase().includes(query)
      && !skill.description.toLowerCase().includes(query)
    ) {
      return false;
    }
    return agentFilter === null || getSkillAssociatedAgentIds(skill).includes(agentFilter);
  });
}

export function getAgentFilterOptions(
  agents: ResolvedAgent[],
  selectedAgent: AgentId | null,
): ResolvedAgent[] {
  return agents
    .filter((agent) => isAgentDetected(agent) || agentId(agent) === selectedAgent)
    .sort((left, right) => agentDisplayName(left).localeCompare(agentDisplayName(right)));
}

export function countSkillsByAgent(skills: InstalledSkill[]): Map<AgentId, number> {
  const counts = new Map<AgentId, number>();
  for (const skill of skills) {
    for (const id of getSkillAssociatedAgentIds(skill)) {
      counts.set(id, (counts.get(id) ?? 0) + 1);
    }
  }
  return counts;
}
