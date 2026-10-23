import type { AgentId, InstalledSkill, ResolvedAgent, SourceFilterInfo } from '@/bindings';
import { agentDisplayName, agentId, isAgentDetected } from '@/lib/agents';

/** 返回 Backend 投影出的、当前可用于 Skill card 展示和筛选的 Agent。 */
export function getSkillAssociatedAgentIds(skill: InstalledSkill): AgentId[] {
  return Array.from(new Set(skill.associatedAgents));
}

export function filterSkills<T extends InstalledSkill>(
  skills: T[],
  searchQuery: string,
  agentFilter: AgentId | null,
  sourceFilter: string | null = null,
): T[] {
  const query = searchQuery.trim().toLowerCase();
  if (!query && agentFilter === null && sourceFilter === null) return skills;

  return skills.filter((skill) => {
    if (sourceFilter !== null && sourceInfo(skill).key !== sourceFilter) return false;
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

export interface SourceFilterOption extends SourceFilterInfo { count: number }

const LOCAL_SOURCE: SourceFilterInfo = { key: 'source-filter:local', kind: 'local', label: '', detail: '' };

function sourceInfo(skill: InstalledSkill): SourceFilterInfo {
  return skill.sourceFilter ?? LOCAL_SOURCE;
}

export function getSourceFilterOptions(skills: InstalledSkill[]): SourceFilterOption[] {
  const sources = new Map<string, SourceFilterOption>();
  for (const skill of skills) {
    const source = sourceInfo(skill);
    const existing = sources.get(source.key);
    if (existing) existing.count += 1;
    else sources.set(source.key, { ...source, count: 1 });
  }
  return [...sources.values()].sort((a, b) => a.label.localeCompare(b.label) || a.key.localeCompare(b.key));
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
