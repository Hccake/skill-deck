import { toast } from 'sonner';
import { previewRemove, removeSkill } from '@/hooks/useTauriApi';
import { getSkillIdentity, isSameSkillIdentity } from '@/lib/skills/identity';
import { useSkillDetailStore } from '@/stores/skill-detail';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { useMutationStore } from '@/stores/mutation';
import { appendCrossStorageFailureGuidance } from '@/utils/cross-storage-guidance';
import { t } from '@/stores/skills-utils';
import type { ContextRef, InstalledSkill, RemoveSelection } from '@/bindings';
import { formatWorkflowError, presentMutationResults } from './mutation-presentation';

let removalPreviewGeneration = 0;

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
  } catch (error) {
    if (requestGeneration === removalPreviewGeneration) {
      console.warn('Failed to preview removal:', error);
    }
  } finally {
    const current = useSkillDialogStore.getState();
    if (requestGeneration === removalPreviewGeneration && current.deleteTarget?.skill === skill) {
      current.setDeleteLoading(false);
    }
  }
}

export async function executeSkillRemoval(selection: RemoveSelection): Promise<void> {
  if (useMutationStore.getState().activeMutation) return;
  const { deleteTarget, deletePreview } = useSkillDialogStore.getState();
  if (!deleteTarget || !deletePreview) return;

  try {
    const context = deleteTarget.context;
    const result = await removeSkill({
      token: deletePreview.token,
      context,
      skillName: deleteTarget.skill.name,
      selection,
    });
    const failed = result.units.filter((unit) => unit.status !== 'succeeded');
    if (failed.length > 0) {
      const presentation = presentMutationResults(failed, t);
      toast.error(appendCrossStorageFailureGuidance(
        presentation.summary,
        context,
        'delete',
        t,
      ));
      return;
    }

    toast.success(selection.removeCanonical
      ? t('skills.deleteSuccess', { name: deleteTarget.skill.name })
      : t('skills.partialDeleteSuccess', {
          name: deleteTarget.skill.name,
          count: selection.entryIds.length,
        }));

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
    await useSkillsDataStore.getState().syncSkills(context);
  } catch (error) {
    toast.error(appendCrossStorageFailureGuidance(
      t('skills.deleteError', {
        name: deleteTarget.skill.name,
        error: formatWorkflowError(error, t),
      }),
      deleteTarget.context,
      'delete',
      t,
    ));
    useSkillDialogStore.getState().closeDelete();
  }
}
