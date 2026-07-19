import { toast } from 'sonner';
import {
  manageSkillAgents,
  previewManageSkillAgents,
} from '@/hooks/useTauriApi';
import { buildAgentWriteIntents } from '@/lib/install-workflow';
import { useMutationStore } from '@/stores/mutation';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { t } from '@/stores/skills-utils';
import { appendCrossStorageFailureGuidance } from '@/utils/cross-storage-guidance';
import type {
  AgentId,
  ContextRef,
  InstalledSkill,
  InstallMode,
  ObservedEntryId,
} from '@/bindings';
import { formatWorkflowError, presentMutationResults } from './mutation-presentation';

let managePreviewGeneration = 0;

export async function openManageAgentChanges(
  skill: InstalledSkill,
  context: ContextRef,
  projectPath?: string,
): Promise<void> {
  const requestGeneration = ++managePreviewGeneration;
  const dialogs = useSkillDialogStore.getState();
  dialogs.openManageAgents(skill, context, projectPath);
  try {
    const preview = await previewManageSkillAgents({
      context,
      skillName: skill.name,
      add: [],
      removeEntryIds: [],
      requestedMode: 'copy',
    });
    const current = useSkillDialogStore.getState();
    if (requestGeneration !== managePreviewGeneration || current.manageAgentsSkill !== skill) return;
    current.setManageAgentDetails(preview);
  } catch (error) {
    if (requestGeneration === managePreviewGeneration) {
      console.warn('Failed to preview Agent management:', error);
    }
  } finally {
    const current = useSkillDialogStore.getState();
    if (requestGeneration === managePreviewGeneration && current.manageAgentsSkill === skill) {
      current.setManageAgentLoading(false);
    }
  }
}

export async function executeManageAgentChanges(
  addAgents: AgentId[],
  removeEntryIds: ObservedEntryId[],
  mode: InstallMode,
  addOptionalAgents: AgentId[],
): Promise<void> {
  if (useMutationStore.getState().activeMutation) return;
  const { manageAgentsSkill, manageAgentsContext, manageAgentDetails } = useSkillDialogStore.getState();
  if (!manageAgentsSkill || !manageAgentsContext || !manageAgentDetails) return;

  const context = manageAgentsContext;
  try {
    const add = buildAgentWriteIntents({
      agents: manageAgentDetails.availableAgents,
      scope: context.scope.scope,
      selectedAgents: addAgents,
      privateCopyAgents: addOptionalAgents,
      adapterTargets: [],
    });
    const preview = await previewManageSkillAgents({
      context,
      skillName: manageAgentsSkill.name,
      add,
      removeEntryIds,
      requestedMode: mode,
    });
    const result = await manageSkillAgents({
      token: preview.token,
      context,
      skillName: manageAgentsSkill.name,
      add,
      removeEntryIds,
      requestedMode: mode,
      confirmEntityDirectories: manageAgentDetails.observedEntries.some(
        (entry) => removeEntryIds.includes(entry.entryId) && entry.kind === 'directory',
      ),
      canonicalPayload: preview.canonicalPayload,
    });
    const presentation = presentMutationResults(result.units, t);

    if (presentation.failedUnits.length > 0) {
      toast.error(appendCrossStorageFailureGuidance(
        presentation.summary,
        context,
        'manageAgents',
        t,
      ));
    } else {
      toast.success(t('skills.manageAgents.success'));
    }

    useSkillDialogStore.getState().closeManageAgents();
    const { useSkillsDataStore } = await import('@/stores/skills-data');
    await useSkillsDataStore.getState().syncSkills(context);
  } catch (error) {
    console.error('[executeManageAgentChanges] Failed:', error);
    toast.error(appendCrossStorageFailureGuidance(
      formatWorkflowError(error, t),
      context,
      'manageAgents',
      t,
    ));
  }
}
