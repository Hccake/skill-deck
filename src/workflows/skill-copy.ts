import {
  copySkillToProjects,
  getCopyAgentSelection,
  previewCopySkillToProjects,
} from '@/hooks/useTauriApi';
import { isBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { toAppError } from '@/utils/to-app-error';
import type {
  AgentSelectionSubmission,
  AppError,
  CopyAgentSelectionSnapshot,
  CopyResponse,
  EnvironmentRef,
  MutationUnitResult,
  RecoveryAction,
} from '@/bindings';
import { runBusinessWrite } from './install-session-feedback';

export interface SkillCopySelection {
  environment: EnvironmentRef;
  projectIds: string[];
  agentSelection: AgentSelectionSubmission;
}

export type CopyOutcome =
  | { status: 'blocked' }
  | { status: 'selectionStale'; snapshot: CopyAgentSelectionSnapshot }
  | { status: 'failed'; error: AppError; unit?: never }
  | { status: 'failed'; unit: MutationUnitResult; error?: never }
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
  agentSelection,
}: SkillCopySelection): Promise<CopyOutcome> {
  if (isBusinessWriteBlocked()) return { status: 'blocked' };
  const { copySkill, copyContext } = useSkillDialogStore.getState();
  if (!copySkill || !copyContext) return { status: 'blocked' };

  try {
    if (copyContext.scope.scope !== 'project' || targetProjectIds.length === 0) {
      throw new Error('Selected projects are not available in the current environment');
    }
    const request = {
      skillName: copySkill.name,
      source: copyContext,
      targetEnvironment,
      targetProjectIds,
      agentSelection,
    };
    const previewOutcome = await previewCopySkillToProjects(request);
    if (previewOutcome.status === 'selectionStale') {
      return previewOutcome;
    }
    const preview = previewOutcome.preview;
    const outcome = await runBusinessWrite(() => copySkillToProjects({
      request,
      token: preview.token,
      payload: preview.payload,
    }));
    if (outcome.status === 'notRun') return { status: 'blocked' };
    const response = outcome.value;

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
    const failedProjectIds = failed
      .map(projectIdOf)
      .filter((projectId): projectId is string => projectId !== null);
    const retryableProjectIds = ordinaryFailed
      .filter((unit) => unit.retryable)
      .map(projectIdOf)
      .filter((projectId): projectId is string => projectId !== null);
    if (targetProjectIds.length === 1) {
      const unit = ordinaryFailed[0] ?? failed[0];
      return {
        status: 'failed',
        unit,
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
    const appError = toAppError(error);
    if (
      appError.kind === 'staleContext'
      || appError.kind === 'staleRegistry'
      || appError.kind === 'staleEnvironment'
    ) {
      try {
        const snapshot = await getCopyAgentSelection(copyContext, copySkill.name);
        if (snapshot.selection.revision !== agentSelection.revision) {
          return { status: 'selectionStale', snapshot };
        }
      } catch {
        // 保留触发执行失败的原始错误，刷新失败不应覆盖它。
      }
    }
    return { status: 'failed', error: appError };
  }
}
