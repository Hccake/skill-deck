import type {
  AgentId,
  DefaultTargetAgents,
  ResolvedAgent,
  ResolvedAgentScope,
} from '@/bindings';
import { agentId, isAgentDetected } from './agents';

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

export function getAgentTarget(
  agent: ResolvedAgent,
  scope: InstallScope,
): ResolvedAgentScope {
  return agent[scope];
}

export function getAgentDisplayPath(
  agent: ResolvedAgent,
  scope: InstallScope,
): string | null {
  const target = getAgentTarget(agent, scope);
  return target.privatePath ?? target.sharedPath;
}

export function getAgentInstallPath(
  agent: ResolvedAgent,
  scope: InstallScope,
): string | null {
  const target = getAgentTarget(agent, scope);
  return target.readsShared ? target.sharedPath : target.privatePath;
}

export function isDefaultAvailableAgent(
  agent: ResolvedAgent,
  scope: InstallScope,
): boolean {
  const target = getAgentTarget(agent, scope);
  return target.enabled && target.readsShared;
}

export function isPrivateRequiredAgent(
  agent: ResolvedAgent,
  scope: InstallScope,
): boolean {
  const target = getAgentTarget(agent, scope);
  return target.enabled && !target.readsShared && target.privatePath !== null;
}

export function canCreatePrivateCopy(
  agent: ResolvedAgent,
  scope: InstallScope,
): boolean {
  const target = getAgentTarget(agent, scope);
  return target.enabled && target.readsShared && target.privatePath !== null;
}

export const isAutomaticAgent = isDefaultAvailableAgent;
export const isAdditionalAgent = isPrivateRequiredAgent;

export interface ScopedAgentGroups {
  detectedDefaultAvailable: ResolvedAgent[];
  undetectedDefaultAvailable: ResolvedAgent[];
  notDetectedDefaultAvailable: ResolvedAgent[];
  indeterminateDefaultAvailable: ResolvedAgent[];
  visibleDefaultAvailableAgents: ResolvedAgent[];
  hiddenDefaultAvailableAgents: ResolvedAgent[];
  detectedPrivateRequired: ResolvedAgent[];
  visiblePrivateRequiredAgents: ResolvedAgent[];
  hiddenPrivateRequiredAgents: ResolvedAgent[];
  privateCopyEligibleAgents: ResolvedAgent[];
  detectedAutomatic: ResolvedAgent[];
  undetectedAutomatic: ResolvedAgent[];
  detectedSelectableAgents: ResolvedAgent[];
  visibleSelectableAgents: ResolvedAgent[];
  hiddenSelectableAgents: ResolvedAgent[];
  selectableCount: number;
}

export function shouldDisplayAgentInitially(
  agent: ResolvedAgent,
  explicitlySelected = false,
): boolean {
  return isAgentDetected(agent)
    || agent.definition.source === 'custom'
    || explicitlySelected;
}

export function groupAgentsByScopedTarget(
  agents: ResolvedAgent[],
  scope: InstallScope,
  selectedAgentIds: ReadonlySet<AgentId> = new Set(),
): ScopedAgentGroups {
  const detectedDefaultAvailable: ResolvedAgent[] = [];
  const undetectedDefaultAvailable: ResolvedAgent[] = [];
  const notDetectedDefaultAvailable: ResolvedAgent[] = [];
  const indeterminateDefaultAvailable: ResolvedAgent[] = [];
  const visibleDefaultAvailableAgents: ResolvedAgent[] = [];
  const hiddenDefaultAvailableAgents: ResolvedAgent[] = [];
  const detectedPrivateRequired: ResolvedAgent[] = [];
  const visiblePrivateRequiredAgents: ResolvedAgent[] = [];
  const hiddenPrivateRequiredAgents: ResolvedAgent[] = [];
  const privateCopyEligibleAgents: ResolvedAgent[] = [];
  let selectableCount = 0;

  for (const agent of agents) {
    const target = getAgentTarget(agent, scope);
    if (!target.enabled) continue;

    if (target.readsShared) {
      switch (agent.detection) {
        case 'detected':
          detectedDefaultAvailable.push(agent);
          break;
        case 'notDetected':
          notDetectedDefaultAvailable.push(agent);
          undetectedDefaultAvailable.push(agent);
          break;
        case 'indeterminate':
          indeterminateDefaultAvailable.push(agent);
          undetectedDefaultAvailable.push(agent);
          break;
      }

      if (shouldDisplayAgentInitially(agent, selectedAgentIds.has(agentId(agent)))) {
        visibleDefaultAvailableAgents.push(agent);
      } else {
        hiddenDefaultAvailableAgents.push(agent);
      }

      if (canCreatePrivateCopy(agent, scope)) {
        privateCopyEligibleAgents.push(agent);
      }
      continue;
    }

    selectableCount += 1;

    if (isAgentDetected(agent)) {
      detectedPrivateRequired.push(agent);
    }

    if (shouldDisplayAgentInitially(agent, selectedAgentIds.has(agentId(agent)))) {
      visiblePrivateRequiredAgents.push(agent);
    } else {
      hiddenPrivateRequiredAgents.push(agent);
    }
  }

  return {
    detectedDefaultAvailable,
    undetectedDefaultAvailable,
    notDetectedDefaultAvailable,
    indeterminateDefaultAvailable,
    visibleDefaultAvailableAgents,
    hiddenDefaultAvailableAgents,
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
  ids: AgentId[],
  agents: ResolvedAgent[],
  scope: InstallScope,
): AgentId[] {
  const agentById = new Map<AgentId, ResolvedAgent>(
    agents.map((agent) => [agentId(agent), agent])
  );
  const result: AgentId[] = [];

  for (const id of ids) {
    const agent = agentById.get(id);
    if (!agent) continue;

    if (!isAdditionalAgent(agent, scope)) continue;
    if (!result.includes(id)) result.push(id);
  }

  return result;
}

export function migrateDefaultTargetAgents(
  lastSelectedAgents: AgentId[],
  agents: ResolvedAgent[],
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
