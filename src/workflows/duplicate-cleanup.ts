import { toast } from 'sonner';
import {
  cleanupDuplicateAgentCopies,
  previewManageSkillAgents,
} from '@/hooks/useTauriApi';
import { isBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { t } from '@/stores/skills-utils';
import { appendCrossStorageFailureGuidance } from '@/utils/cross-storage-guidance';
import type { AgentId } from '@/bindings';
import { formatWorkflowError } from './mutation-presentation';

export async function executeDuplicateCleanup(agents: AgentId[]): Promise<void> {
  if (isBusinessWriteBlocked() || agents.length === 0) return;
  const { manageAgentsSkill, manageAgentsContext } = useSkillDialogStore.getState();
  if (!manageAgentsSkill || !manageAgentsContext) return;

  const context = manageAgentsContext;
  try {
    const results = await cleanupDuplicateAgentCopies(context, {
      skillName: manageAgentsSkill.name,
      agents,
    });
    const failures = results.filter((result) => !result.success && !result.skipped);
    if (failures.length > 0) {
      toast.error(appendCrossStorageFailureGuidance(
        failures.map((result) => (
          `${result.agent}: ${t(`mutation.result.errors.${result.error ?? 'unknown'}`)}`
        )).join('\n'),
        context,
        'cleanup',
        t,
      ));
    } else {
      toast.success(t('skills.manageAgents.cleanupSuccess'));
    }

    const [details] = await Promise.all([
      previewManageSkillAgents({
        context,
        skillName: manageAgentsSkill.name,
        add: [],
        removeEntryIds: [],
        requestedMode: 'copy',
      }),
      import('@/stores/skills-data').then(({ useSkillsDataStore }) => (
        useSkillsDataStore.getState().syncSkills(context, results.some((result) => result.success)
          ? { origin: 'selfMutation', mutatedSkillNames: [manageAgentsSkill.name] }
          : { origin: 'passive' })
      )),
    ]);
    useSkillDialogStore.setState({ manageAgentDetails: details });
  } catch (error) {
    console.error('[executeDuplicateCleanup] Failed:', error);
    toast.error(appendCrossStorageFailureGuidance(
      formatWorkflowError(error, t),
      context,
      'cleanup',
      t,
    ));
  }
}
