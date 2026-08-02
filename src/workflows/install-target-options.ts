import type {
  AgentId,
  AgentSelectionGroup,
  InstallMode,
  InstallTargetInfo,
  ResolvedAgent,
} from '@/bindings';
import {
  canCreatePrivateCopy,
  filterAdditionalAgentIds,
  migrateDefaultTargetAgents,
  type InstallScope,
} from '@/lib/agentTargets';
import { agentId, isAgentDetected } from '@/lib/agents';
import type { AdapterTargetSelection } from '@/lib/install-workflow';

const DEFAULT_NON_UNIVERSAL_AGENTS: AgentId[] = ['claude-code', 'cursor'];

export interface InstallTargetFacts {
  allAgents: ResolvedAgent[];
  selectionGroups: AgentSelectionGroup[];
  availableAgentTargets: InstallTargetInfo[];
  defaultAgents: AgentId[] | null;
  defaultsUnavailable: boolean;
}

export interface InstallTargetSelection {
  selectedAgents: AgentId[];
  privateCopyAgents: AgentId[];
  selectedAgentTargets: AdapterTargetSelection[];
  mode: InstallMode;
}

export function initializeInstallTargetSelection({
  scope,
  preselectedAgents,
  mode,
  facts,
}: {
  scope: InstallScope;
  preselectedAgents: AgentId[];
  mode: InstallMode;
  facts: InstallTargetFacts;
}): InstallTargetSelection {
  const fallbackAgents = migrateDefaultTargetAgents(
    DEFAULT_NON_UNIVERSAL_AGENTS,
    facts.allAgents,
  )[scope].filter((id) => (
    facts.allAgents.some((agent) => agentId(agent) === id && isAgentDetected(agent))
  ));
  const selectedAgents = filterAdditionalAgentIds(
    preselectedAgents.length > 0
      ? preselectedAgents
      : facts.defaultAgents ?? fallbackAgents,
    facts.allAgents,
    scope,
  );
  const selectedAgentIds = new Set(selectedAgents);

  return {
    selectedAgents,
    privateCopyAgents: [],
    selectedAgentTargets: facts.availableAgentTargets
      .filter((target) => selectedAgentIds.has(target.agent))
      .map(targetSelection),
    mode,
  };
}

export function reconcileInstallTargetSelection({
  scope,
  selection,
  facts,
}: {
  scope: InstallScope;
  selection: InstallTargetSelection;
  facts: InstallTargetFacts;
}): InstallTargetSelection {
  const agentsById = new Map(
    facts.allAgents.map((agent) => [agentId(agent), agent]),
  );
  const availableTargetsById = new Map(
    facts.availableAgentTargets.map((target) => [target.targetId, target]),
  );
  const selectedAgents = filterAdditionalAgentIds(
    selection.selectedAgents,
    facts.allAgents,
    scope,
  );
  const selectedAgentIds = new Set(selectedAgents);
  const seenTargetIds = new Set<string>();

  return {
    selectedAgents,
    privateCopyAgents: selection.privateCopyAgents.filter((id, index, ids) => {
      const agent = agentsById.get(id);
      return ids.indexOf(id) === index
        && agent !== undefined
        && canCreatePrivateCopy(agent, scope);
    }),
    selectedAgentTargets: selection.selectedAgentTargets.flatMap((selectionTarget) => {
      const availableTarget = availableTargetsById.get(selectionTarget.targetId);
      if (!availableTarget
        || seenTargetIds.has(availableTarget.targetId)
        || !selectedAgentIds.has(availableTarget.agent)) {
        return [];
      }
      seenTargetIds.add(availableTarget.targetId);
      return [targetSelection(availableTarget)];
    }),
    mode: selection.mode,
  };
}

function targetSelection(target: InstallTargetInfo): AdapterTargetSelection {
  return {
    agentId: target.agent,
    targetId: target.targetId,
  };
}
