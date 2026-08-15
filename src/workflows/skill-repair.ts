import { fetchAvailable, getInstallAgentSelection, installSkills } from '@/hooks/useTauriApi';
import type {
  AgentId,
  AppError,
  SkillLocationRef,
  FetchResult,
  InstallResponse,
  RecoveryAction,
} from '@/bindings';
import { isBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import { preserveOwnDirectoryOptions } from '@/lib/agent-selection-session';
import { toAppError } from '@/utils/to-app-error';
import { prepareInstall, type InstallPreparationOutcome } from './skill-install-preparation';
import { runBusinessWrite } from './install-session-feedback';

export interface RepairSkillSourceRequest {
  context: SkillLocationRef;
  source: string;
  skillName: string;
  agents?: AgentId[];
  privateAdaptedAgents?: AgentId[];
  privateCopyAgents?: AgentId[];
  operationId: string;
  stopRequested: () => boolean;
  onPhase?: (phase: 'validating' | 'preparing' | 'installing') => void;
}

export type RepairOutcome =
  | { status: 'blocked' }
  | { status: 'succeeded'; response: InstallResponse }
  | { status: 'stopped' }
  | { status: 'missing' }
  | { status: 'recoveryRequired'; response: InstallResponse; recovery: RecoveryAction[] }
  | {
    status: 'failed';
    stage: 'validation' | 'preparation' | 'execution';
    error: AppError | null;
  };

export interface RepairWorkflowApi {
  fetchAvailable: typeof fetchAvailable;
  prepareInstall: typeof prepareInstall;
  installSkills: typeof installSkills;
  getInstallAgentSelection: typeof getInstallAgentSelection;
}

const defaultApi: RepairWorkflowApi = {
  fetchAvailable,
  prepareInstall,
  installSkills,
  getInstallAgentSelection,
};

function uniqueAgentIds(agents: AgentId[] | undefined): AgentId[] {
  return Array.from(new Set(agents ?? []));
}

function isSuccessful(response: InstallResponse): boolean {
  return response.units.length > 0
    && response.units.every((unit) => unit.status === 'succeeded');
}

function recoveryActions(response: InstallResponse): RecoveryAction[] {
  const seen = new Set<string>();
  return response.units.flatMap((unit) => {
    const recovery = unit.recovery;
    if (!recovery || seen.has(recovery.resourceId)) return [];
    seen.add(recovery.resourceId);
    return [recovery];
  });
}

export async function repairSkillSource(
  request: RepairSkillSourceRequest,
  api: RepairWorkflowApi = defaultApi,
): Promise<RepairOutcome> {
  if (isBusinessWriteBlocked()) return { status: 'blocked' };
  if (request.stopRequested()) return { status: 'stopped' };
  request.onPhase?.('validating');

  let available: FetchResult;
  try {
    available = await api.fetchAvailable(request.context, request.source.trim(), request.operationId);
  } catch (error) {
    return { status: 'failed', stage: 'validation', error: toAppError(error) };
  }
  if (request.stopRequested()) return { status: 'stopped' };

  const skill = available.skills.find((item) => item.name === request.skillName);
  if (!skill) return { status: 'missing' };
  let preparation: InstallPreparationOutcome;
  request.onPhase?.('preparing');
  try {
    const requestedAgents = uniqueAgentIds([
      ...(request.privateAdaptedAgents ?? request.agents ?? []),
      ...(request.privateCopyAgents ?? []),
    ]);
    const agentSnapshot = await api.getInstallAgentSelection(request.context, requestedAgents);
    preparation = await api.prepareInstall({
      context: request.context,
      source: request.source.trim(),
      discoverySession: available.discoverySession,
      skillPaths: [skill.relativePath],
      skills: [request.skillName],
      explicitAgentIds: requestedAgents,
      agentSelection: {
        revision: agentSnapshot.selection.revision,
        selectedOptionIds: preserveOwnDirectoryOptions(
          agentSnapshot.selection,
          requestedAgents,
        ),
        requestedMode: 'copy',
      },
      acknowledgeRedirect: false,
    });
  } catch (error) {
    return { status: 'failed', stage: 'preparation', error: toAppError(error) };
  }
  if (request.stopRequested()) return { status: 'stopped' };
  if (preparation.status === 'failed') {
    return { status: 'failed', stage: 'preparation', error: preparation.error };
  }
  if (preparation.status === 'selectionStale') {
    return { status: 'failed', stage: 'preparation', error: { kind: 'staleTarget' } };
  }

  let response: InstallResponse;
  request.onPhase?.('installing');
  try {
    const outcome = await runBusinessWrite(() => (
      api.installSkills(preparation.prepared.request, preparation.prepared.preview.token)
    ));
    if (outcome.status === 'notRun') return { status: 'blocked' };
    response = outcome.value;
  } catch (error) {
    if (request.stopRequested()) return { status: 'stopped' };
    return { status: 'failed', stage: 'execution', error: toAppError(error) };
  }
  if (request.stopRequested()) return { status: 'stopped' };
  const recoveries = recoveryActions(response);
  if (recoveries.length > 0) {
    return { status: 'recoveryRequired', response, recovery: recoveries };
  }
  return isSuccessful(response)
    ? { status: 'succeeded', response }
    : { status: 'failed', stage: 'execution', error: null };
}
