import {
  copySkillToProjects,
  previewCopySkillToProjects,
} from '@/hooks/useTauriApi';
import { useMutationStore } from '@/stores/mutation';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { getSkillOperationAgents } from '@/stores/skills-utils';
import { toAppError } from '@/utils/to-app-error';
import type { AppError, CopyResponse, EnvironmentRef, MutationUnitResult } from '@/bindings';

export interface SkillCopySelection {
  environment: EnvironmentRef;
  projectIds: string[];
}

export type CopyOutcome =
  | { status: 'blocked' }
  | { status: 'failed'; error: AppError }
  | { status: 'succeeded'; response: CopyResponse; succeededProjectIds: string[] }
  | {
    status: 'partial';
    response: CopyResponse;
    succeededProjectIds: string[];
    failedProjectIds: string[];
    retryableProjectIds: string[];
  };

function projectIdOf(unit: MutationUnitResult): string | null {
  if (!unit.target) return null;
  return unit.target.scope.scope === 'project' ? unit.target.scope.project_id : null;
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
    const preview = await previewCopySkillToProjects(request);
    const response = await copySkillToProjects({
      request,
      token: preview.token,
      payload: preview.payload,
    });

    const succeededProjectIds = response.units
      .filter((unit) => unit.status === 'succeeded')
      .map(projectIdOf)
      .filter((projectId): projectId is string => projectId !== null);
    const failed = response.units.filter((unit) => unit.status !== 'succeeded');
    if (failed.length === 0) {
      return { status: 'succeeded', response, succeededProjectIds };
    }
    const failedProjectIds = failed
      .map(projectIdOf)
      .filter((projectId): projectId is string => projectId !== null);
    const retryableProjectIds = failed
      .filter((unit) => unit.retryable)
      .map(projectIdOf)
      .filter((projectId): projectId is string => projectId !== null);
    return {
      status: 'partial',
      response,
      succeededProjectIds,
      failedProjectIds,
      retryableProjectIds,
    };
  } catch (error) {
    return { status: 'failed', error: toAppError(error) };
  }
}
