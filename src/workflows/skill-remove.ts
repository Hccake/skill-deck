import { toast } from 'sonner';
import { previewRemove, removeSkill } from '@/hooks/useTauriApi';
import { getSkillIdentity, isSameSkillIdentity } from '@/lib/skills/identity';
import { useSkillDetailStore } from '@/stores/skill-detail';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { isBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import { appendCrossStorageFailureGuidance } from '@/utils/cross-storage-guidance';
import { t, type DeleteTarget } from '@/stores/skills-utils';
import type { ContextRef, InstalledSkill, RecoveryAction } from '@/bindings';
import { formatWorkflowError, presentMutationResults } from './mutation-presentation';
import { runBusinessWrite } from './install-session-feedback';

let removalPreviewGeneration = 0;

export type SkillRemovalOutcome =
  | { status: 'succeeded' }
  | { status: 'failed' }
  | { status: 'stale' }
  | { status: 'notRun' }
  | { status: 'recoveryRequired'; recovery: RecoveryAction[] };

const STALE_REMOVAL_CODES = new Set([
  'staleContext',
  'staleRegistry',
  'staleEnvironment',
  'staleTarget',
  'externalLockChanged',
]);

function hasStaleRemovalResult(units: Awaited<ReturnType<typeof removeSkill>>['units']): boolean {
  return units.some((unit) => unit.error && STALE_REMOVAL_CODES.has(unit.error.code));
}

function isStaleRemovalError(error: unknown): boolean {
  return Boolean(
    error
      && typeof error === 'object'
      && 'kind' in error
      && typeof error.kind === 'string'
      && (STALE_REMOVAL_CODES.has(error.kind) || error.kind === 'staleAgentRuntime'),
  );
}

async function reloadSkillRemoval(target: DeleteTarget): Promise<void> {
  await openSkillRemoval(target.skill, target.context, target.projectPath);
  const current = useSkillDialogStore.getState();
  if (current.deleteTarget?.skill === target.skill && current.deletePreview) {
    current.setDeleteFeedback('stale');
  }
}

export async function openSkillRemoval(
  skill: InstalledSkill,
  context: ContextRef,
  projectPath?: string,
): Promise<void> {
  const requestGeneration = ++removalPreviewGeneration;
  const dialogs = useSkillDialogStore.getState();
  dialogs.openDelete(skill, context, projectPath);
  try {
    const preview = await previewRemove(context, skill.name);
    const current = useSkillDialogStore.getState();
    if (requestGeneration !== removalPreviewGeneration || current.deleteTarget?.skill !== skill) return;
    current.setDeletePreview(preview);
  } catch {
    if (requestGeneration === removalPreviewGeneration) {
      useSkillDialogStore.getState().setDeleteFeedback('previewError');
    }
  } finally {
    const current = useSkillDialogStore.getState();
    if (requestGeneration === removalPreviewGeneration && current.deleteTarget?.skill === skill) {
      current.setDeleteLoading(false);
    }
  }
}

export async function executeSkillRemoval(): Promise<SkillRemovalOutcome> {
  if (isBusinessWriteBlocked()) return { status: 'notRun' };
  const { deleteTarget, deletePreview } = useSkillDialogStore.getState();
  if (!deleteTarget || !deletePreview) return { status: 'notRun' };
  useSkillDialogStore.getState().setDeleteFeedback(null);

  try {
    const context = deleteTarget.context;
    const outcome = await runBusinessWrite(() => removeSkill({
      token: deletePreview.token,
      context,
      skillName: deleteTarget.skill.name,
      intent: { kind: 'fullSkill' },
    }));
    if (outcome.status === 'notRun') return { status: 'notRun' };
    const result = outcome.value;
    const failed = result.units.filter((unit) => unit.status !== 'succeeded');
    if (failed.length > 0) {
      const recovery = failed.flatMap((unit) => (
        unit.status === 'recoveryRequired' && unit.recovery ? [unit.recovery] : []
      ));
      if (recovery.length > 0) {
        return { status: 'recoveryRequired', recovery };
      }
      if (hasStaleRemovalResult(failed)) {
        await reloadSkillRemoval(deleteTarget);
        return { status: 'stale' };
      }
      const presentation = presentMutationResults(failed, t);
      useSkillDialogStore.getState().setDeleteFeedback('executionError');
      toast.error(appendCrossStorageFailureGuidance(
        presentation.summary,
        context,
        'delete',
        t,
      ));
      return { status: 'failed' };
    }

    toast.success(t('skills.deleteSuccess', { name: deleteTarget.skill.name }));

    const detailState = useSkillDetailStore.getState();
    const deletedSkillIdentity = getSkillIdentity(
      deleteTarget.skill,
      deleteTarget.scope === 'project' ? deleteTarget.projectPath : undefined,
    );
    if (isSameSkillIdentity(detailState.selectedSkillRef, deletedSkillIdentity)) {
      detailState.deselectSkill();
    }

    useSkillDialogStore.getState().closeDelete();
    const { useSkillsDataStore } = await import('@/stores/skills-data');
    await useSkillsDataStore.getState().syncSkills(context, {
      origin: 'selfMutation',
      mutatedSkillNames: [deleteTarget.skill.name],
    });
    return { status: 'succeeded' };
  } catch (error) {
    if (isStaleRemovalError(error)) {
      await reloadSkillRemoval(deleteTarget);
      return { status: 'stale' };
    }
    useSkillDialogStore.getState().setDeleteFeedback('executionError');
    toast.error(appendCrossStorageFailureGuidance(
      t('skills.deleteError', {
        name: deleteTarget.skill.name,
        error: formatWorkflowError(error, t),
      }),
      deleteTarget.context,
      'delete',
      t,
    ));
    return { status: 'failed' };
  }
}
