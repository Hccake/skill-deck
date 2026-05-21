import type { AgentInfo, DefaultTargetAgents } from '@/bindings';

export type InstallScope = 'global' | 'project';

export const EMPTY_DEFAULT_TARGET_AGENTS: DefaultTargetAgents = {
  global: [],
  project: [],
};

const SHARED_SKILL_DIRECTORIES: Record<InstallScope, string> = {
  global: '~/.agents/skills',
  project: './.agents/skills',
};

export function getSharedSkillDirectory(scope: InstallScope) {
  return SHARED_SKILL_DIRECTORIES[scope];
}

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

export interface ScopedAgentGroups {
  detectedAutomatic: AgentInfo[];
  undetectedAutomatic: AgentInfo[];
  detectedSelectableAgents: AgentInfo[];
  visibleSelectableAgents: AgentInfo[];
  hiddenSelectableAgents: AgentInfo[];
  selectableCount: number;
}

export function groupAgentsByScopedTarget(
  agents: AgentInfo[],
  scope: InstallScope,
  selectedAgentIds: ReadonlySet<string> = new Set(),
): ScopedAgentGroups {
  const detectedAutomatic: AgentInfo[] = [];
  const undetectedAutomatic: AgentInfo[] = [];
  const detectedSelectableAgents: AgentInfo[] = [];
  const visibleSelectableAgents: AgentInfo[] = [];
  const hiddenSelectableAgents: AgentInfo[] = [];
  let selectableCount = 0;

  for (const agent of agents) {
    const target = getAgentTarget(agent, scope);
    if (!target.supported) continue;

    if (target.automatic) {
      if (agent.detected) {
        detectedAutomatic.push(agent);
      } else {
        undetectedAutomatic.push(agent);
      }
      continue;
    }

    selectableCount += 1;

    if (agent.detected) {
      detectedSelectableAgents.push(agent);
    }

    if (agent.detected || selectedAgentIds.has(agent.id)) {
      visibleSelectableAgents.push(agent);
    } else {
      hiddenSelectableAgents.push(agent);
    }
  }

  return {
    detectedAutomatic,
    undetectedAutomatic,
    detectedSelectableAgents,
    visibleSelectableAgents,
    hiddenSelectableAgents,
    selectableCount,
  };
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

function detectDisplayPlatform(): 'win32' | 'posix' {
  if (typeof navigator !== 'undefined' && navigator.platform.toLowerCase().includes('win')) {
    return 'win32';
  }

  return 'posix';
}

export function formatAgentTargetPath(path: string, platform: 'win32' | 'posix' = detectDisplayPlatform()) {
  if (!path) return path;

  const normalizedPath = path.replace(/[\\/]+/g, '/').replace(/\/+$/, '');
  if (!normalizedPath) return path;

  const isAbsolutePath = /^[A-Za-z]:\/|^\/|^\\\\/.test(normalizedPath);
  if (!isAbsolutePath) {
    return normalizedPath;
  }

  if (platform === 'win32') {
    return normalizedPath.replace(/\//g, '\\');
  }

  return normalizedPath;
}
