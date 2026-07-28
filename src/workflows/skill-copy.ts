import {
  copySkillToProjects,
  previewCopySkillToProjects,
} from '@/hooks/useTauriApi';
import { useMutationStore } from '@/stores/mutation';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { getSkillOperationAgents } from '@/stores/skills-utils';
import { toAppError } from '@/utils/to-app-error';
import type {
  AppError,
  CopySourceRepairReason,
  CopyResponse,
  EnvironmentRef,
  MutationUnitResult,
  RecoveryAction,
} from '@/bindings';

export interface SkillCopySelection {
  environment: EnvironmentRef;
  projectIds: string[];
}

export type CopyOutcome =
  | { status: 'blocked' }
  | { status: 'sourceRepairRequired'; reason: CopySourceRepairReason }
  | { status: 'failed'; error: AppError }
  | { status: 'succeeded'; response: CopyResponse; succeededProjectIds: string[] }
  | {
    status: 'recoveryRequired';
    response: CopyResponse;
    succeededProjectIds: string[];
    recovery: RecoveryAction[];
  }
  | {
    status: 'partial';
    response: CopyResponse;
    succeededProjectIds: string[];
    failedProjectIds: string[];
    retryableProjectIds: string[];
    recovery?: RecoveryAction[];
  };

function projectIdOf(unit: MutationUnitResult): string | null {
  if (!unit.target) return null;
  return unit.target.scope.scope === 'project' ? unit.target.scope.project_id : null;
}

function recoveryActions(units: MutationUnitResult[]): RecoveryAction[] {
  const seen = new Set<string>();
  return units.flatMap((unit) => {
    const recovery = unit.recovery;
    if (!recovery || seen.has(recovery.resourceId)) return [];
    seen.add(recovery.resourceId);
    return [recovery];
  });
}

export async function executeSkillCopy({
  environment: targetEnvironment,
  projectIds: targetProjectIds,
}: SkillCopySelection): Promise<CopyOutcome> {
  if (useMutationStore.getState().activeMutation) return { status: 'blocked' };
  const { copySkill, copyContext } = useSkillDialogStore.getState();
  if (!copySkill || !copyContext) return { status: 'blocked' };

  try {
    const agents = copySkill.privateAdaptedAgents ?? getSkillOperationAgents(copySkill);
    const privateCopyAgents = copySkill.privateCopyAgents ?? [];
    if (copyContext.scope.scope !== 'project' || targetProjectIds.length === 0) {
      throw new Error('Selected projects are not available in the current environment');
    }
    const privateSet = new Set(privateCopyAgents);
    const agentIntents = Array.from(new Set([...agents, ...privateCopyAgents])).map((agentId) => ({
      agentId,
      privateEntry: privateSet.has(agentId) ? 'optionalSelected' as const : 'required' as const,
      adapterTargets: [],
    }));
    const request = {
      skillName: copySkill.name,
      source: copyContext,
      targetEnvironment,
      targetProjectIds,
      requestedMode: 'copy' as const,
      agentIntents,
    };
    const previewOutcome = await previewCopySkillToProjects(request);
    if (previewOutcome.status === 'sourceRepairRequired') {
      return previewOutcome;
    }
    const preview = previewOutcome.preview;
    const response = await copySkillToProjects({
      request,
      token: preview.token,
      payload: preview.payload,
    });

    const succeededProjectIds = response.units
      .filter((unit) => unit.status === 'succeeded')
      .map(projectIdOf)
      .filter((projectId): projectId is string => projectId !== null);
    if (succeededProjectIds.length > 0) {
      const { useSkillsDataStore } = await import('@/stores/skills-data');
      const { refreshContext } = useSkillsDataStore.getState();
      await Promise.all(succeededProjectIds.map((projectId) => refreshContext({
        environment: targetEnvironment,
        scope: { scope: 'project', project_id: projectId },
      }, { origin: 'selfMutation', mutatedSkillNames: [copySkill.name] })));
    }
    const failed = response.units.filter((unit) => unit.status !== 'succeeded');
    if (failed.length === 0) {
      return { status: 'succeeded', response, succeededProjectIds };
    }
    const recoveries = recoveryActions(failed);
    const ordinaryFailed = failed.filter((unit) => unit.status !== 'recoveryRequired' || !unit.recovery);
    if (targetProjectIds.length === 1 && ordinaryFailed.length === 0 && recoveries.length > 0) {
      return {
        status: 'recoveryRequired',
        response,
        succeededProjectIds,
        recovery: recoveries,
      };
    }
    const failedProjectIds = ordinaryFailed
      .map(projectIdOf)
      .filter((projectId): projectId is string => projectId !== null);
    const retryableProjectIds = ordinaryFailed
      .filter((unit) => unit.retryable)
      .map(projectIdOf)
      .filter((projectId): projectId is string => projectId !== null);
    if (targetProjectIds.length === 1) {
      return {
        status: 'failed',
        error: toAppError(ordinaryFailed[0]?.error ?? new Error('Copy mutation failed')),
      };
    }
    return {
      status: 'partial',
      response,
      succeededProjectIds,
      failedProjectIds,
      retryableProjectIds,
      recovery: recoveries,
    };
  } catch (error) {
    return { status: 'failed', error: toAppError(error) };
  }
}
