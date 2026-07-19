import { toast } from 'sonner';
import {
  copySkillToProjects,
  previewCopySkillToProjects,
} from '@/hooks/useTauriApi';
import { useMutationStore } from '@/stores/mutation';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { getSkillOperationAgents, t } from '@/stores/skills-utils';
import { appendCrossStorageFailureGuidance } from '@/utils/cross-storage-guidance';
import type { EnvironmentRef } from '@/bindings';
import { formatWorkflowError, presentMutationResults } from './mutation-presentation';

export interface SkillCopySelection {
  environment: EnvironmentRef;
  projectIds: string[];
}

export async function executeSkillCopy({
  environment: targetEnvironment,
  projectIds: targetProjectIds,
}: SkillCopySelection): Promise<void> {
  if (useMutationStore.getState().activeMutation) return;
  const { copySkill, copyContext } = useSkillDialogStore.getState();
  if (!copySkill || !copyContext) return;

  const context = copyContext;
  try {
    const agents = copySkill.privateAdaptedAgents ?? getSkillOperationAgents(copySkill);
    const privateCopyAgents = copySkill.privateCopyAgents ?? [];
    if (context.scope.scope !== 'project' || targetProjectIds.length === 0) {
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
      source: context,
      targetEnvironment,
      targetProjectIds,
      requestedMode: 'copy' as const,
      agentIntents,
    };
    const preview = await previewCopySkillToProjects(request);
    const result = await copySkillToProjects({
      request,
      token: preview.token,
      payload: preview.payload,
    });

    const succeeded = result.units.filter((unit) => unit.status === 'succeeded').length;
    const failed = result.units.filter((unit) => unit.status !== 'succeeded');
    if (failed.length > 0) {
      const errors = failed.map((unit) => appendCrossStorageFailureGuidance(
        presentMutationResults([unit], t).summary,
        unit.target,
        'copy',
        t,
      )).join('\n');
      toast.error(`${t('skills.copyToProject.partialError', {
        success: succeeded,
        fail: failed.length,
      })}\n${errors}`);
    } else {
      toast.success(t('skills.copyToProject.success', { count: succeeded }));
    }

    useSkillDialogStore.getState().closeCopyToProject();
  } catch (error) {
    console.error('[executeSkillCopy] Failed:', error);
    toast.error(appendCrossStorageFailureGuidance(
      formatWorkflowError(error, t),
      context,
      'copy',
      t,
    ));
  }
}
