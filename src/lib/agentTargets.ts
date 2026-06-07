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

export function isDefaultAvailableAgent(agent: AgentInfo, scope: InstallScope): boolean {
  const target = getAgentTarget(agent, scope);
  return target.supported && target.defaultAvailable;
}

export function isPrivateRequiredAgent(agent: AgentInfo, scope: InstallScope): boolean {
  const target = getAgentTarget(agent, scope);
  return target.supported && !target.defaultAvailable;
}

export function canCreatePrivateCopy(agent: AgentInfo, scope: InstallScope): boolean {
  const target = getAgentTarget(agent, scope);
  return target.supported
    && target.defaultAvailable
    && target.privatePath !== null
    && target.availability === 'shared-compatible';
}

export const isAutomaticAgent = isDefaultAvailableAgent;
export const isAdditionalAgent = isPrivateRequiredAgent;

export interface ScopedAgentGroups {
  detectedDefaultAvailable: AgentInfo[];
  undetectedDefaultAvailable: AgentInfo[];
  detectedPrivateRequired: AgentInfo[];
  visiblePrivateRequiredAgents: AgentInfo[];
  hiddenPrivateRequiredAgents: AgentInfo[];
  privateCopyEligibleAgents: AgentInfo[];
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
  const detectedDefaultAvailable: AgentInfo[] = [];
  const undetectedDefaultAvailable: AgentInfo[] = [];
  const detectedPrivateRequired: AgentInfo[] = [];
  const visiblePrivateRequiredAgents: AgentInfo[] = [];
  const hiddenPrivateRequiredAgents: AgentInfo[] = [];
  const privateCopyEligibleAgents: AgentInfo[] = [];
  let selectableCount = 0;

  for (const agent of agents) {
    const target = getAgentTarget(agent, scope);
    if (!target.supported) continue;

    if (target.defaultAvailable) {
      if (agent.detected) {
        detectedDefaultAvailable.push(agent);
      } else {
        undetectedDefaultAvailable.push(agent);
      }

      if (canCreatePrivateCopy(agent, scope)) {
        privateCopyEligibleAgents.push(agent);
      }
      continue;
    }

    selectableCount += 1;

    if (agent.detected) {
      detectedPrivateRequired.push(agent);
    }

    if (agent.detected || selectedAgentIds.has(agent.id)) {
      visiblePrivateRequiredAgents.push(agent);
    } else {
      hiddenPrivateRequiredAgents.push(agent);
    }
  }

  return {
    detectedDefaultAvailable,
    undetectedDefaultAvailable,
    detectedPrivateRequired,
    visiblePrivateRequiredAgents,
    hiddenPrivateRequiredAgents,
    privateCopyEligibleAgents,
    detectedAutomatic: detectedDefaultAvailable,
    undetectedAutomatic: undetectedDefaultAvailable,
    detectedSelectableAgents: detectedPrivateRequired,
    visibleSelectableAgents: visiblePrivateRequiredAgents,
    hiddenSelectableAgents: hiddenPrivateRequiredAgents,
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
