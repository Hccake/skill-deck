import { fetchAvailable, installSkills } from '@/hooks/useTauriApi';
import type {
  AgentId,
  AgentWriteIntent,
  AppError,
  ContextRef,
  FetchResult,
  InstallResponse,
  RecoveryAction,
} from '@/bindings';
import { toAppError } from '@/utils/to-app-error';
import { prepareInstall, type InstallPreparationOutcome } from './skill-install-preparation';

export interface RepairSkillSourceRequest {
  context: ContextRef;
  source: string;
  skillName: string;
  agents?: AgentId[];
  privateAdaptedAgents?: AgentId[];
  privateCopyAgents?: AgentId[];
  acknowledgeRisk: boolean;
  operationId: string;
  stopRequested: () => boolean;
  onPhase?: (phase: 'validating' | 'preparing' | 'installing') => void;
}

export type RepairOutcome =
  | { status: 'succeeded'; response: InstallResponse }
  | { status: 'stopped' }
  | { status: 'missing' }
  | { status: 'riskRequired' }
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
}

const defaultApi: RepairWorkflowApi = {
  fetchAvailable,
  prepareInstall,
  installSkills,
};

function uniqueAgentIds(agents: AgentId[] | undefined): AgentId[] {
  return Array.from(new Set(agents ?? []));
}

function buildAgentIntents(request: RepairSkillSourceRequest): AgentWriteIntent[] {
  const required = uniqueAgentIds(request.privateAdaptedAgents ?? request.agents);
  const copies = new Set(uniqueAgentIds(request.privateCopyAgents));
  return uniqueAgentIds([...required, ...copies]).map((agentId) => ({
    agentId,
    privateEntry: copies.has(agentId) ? 'optionalSelected' : 'required',
    adapterTargets: [],
  }));
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
  if (available.riskPolicy.kind === 'require-confirmation' && !request.acknowledgeRisk) {
    return { status: 'riskRequired' };
  }

  let preparation: InstallPreparationOutcome;
  request.onPhase?.('preparing');
  try {
    preparation = await api.prepareInstall({
      context: request.context,
      source: request.source.trim(),
      discoverySession: available.discoverySession,
      skillPaths: [skill.relativePath],
      skills: [request.skillName],
      agentIntents: buildAgentIntents(request),
      requestedMode: 'copy',
      acknowledgeRisk: available.riskPolicy.kind === 'require-confirmation'
        ? request.acknowledgeRisk
        : true,
    });
  } catch (error) {
    return { status: 'failed', stage: 'preparation', error: toAppError(error) };
  }
  if (request.stopRequested()) return { status: 'stopped' };
  if (preparation.status === 'failed') {
    return { status: 'failed', stage: 'preparation', error: preparation.error };
  }

  let response: InstallResponse;
  request.onPhase?.('installing');
  try {
    response = await api.installSkills(preparation.prepared.request, preparation.prepared.preview.token);
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
