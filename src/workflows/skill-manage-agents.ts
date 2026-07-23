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
  ManageAgentsResponse,
  MutationUnitResult,
  ObservedEntryId,
} from '@/bindings';
import { formatWorkflowError, presentMutationResults } from './mutation-presentation';

let managePreviewGeneration = 0;

const STALE_MANAGE_AGENT_CODES = new Set([
  'staleContext',
  'staleRegistry',
  'staleEnvironment',
  'stalePayload',
  'staleTarget',
  'externalLockChanged',
]);

export type ManageAgentsOutcome =
  | { status: 'blocked' }
  | { status: 'succeeded'; response: ManageAgentsResponse }
  | { status: 'partial'; response: ManageAgentsResponse; message: string }
  | { status: 'stale' }
  | { status: 'failed'; message: string };

function hasStaleManageAgentResult(units: MutationUnitResult[]): boolean {
  return units.some((unit) => unit.error && STALE_MANAGE_AGENT_CODES.has(unit.error.code));
}

function isStaleManageAgentError(error: unknown): boolean {
  return Boolean(
    error
      && typeof error === 'object'
      && 'kind' in error
      && typeof error.kind === 'string'
      && (STALE_MANAGE_AGENT_CODES.has(error.kind) || error.kind === 'staleAgentRuntime'),
  );
}

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
): Promise<ManageAgentsOutcome> {
  if (useMutationStore.getState().activeMutation) return { status: 'blocked' };
  const {
    manageAgentsSkill,
    manageAgentsContext,
    manageAgentsProjectPath,
    manageAgentDetails,
  } = useSkillDialogStore.getState();
  if (!manageAgentsSkill || !manageAgentsContext || !manageAgentDetails) {
    return { status: 'blocked' };
  }

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
    const failedUnits = result.units.filter((unit) => unit.status !== 'succeeded');

    if (hasStaleManageAgentResult(failedUnits)) {
      await openManageAgentChanges(manageAgentsSkill, context, manageAgentsProjectPath);
      return { status: 'stale' };
    }

    if (failedUnits.length > 0) {
      const message = appendCrossStorageFailureGuidance(
        presentation.summary,
        context,
        'manageAgents',
        t,
      );
      const { useSkillsDataStore } = await import('@/stores/skills-data');
      await useSkillsDataStore.getState().syncSkills(context);
      return result.units.some((unit) => unit.status === 'succeeded')
        ? { status: 'partial', response: result, message }
        : { status: 'failed', message };
    }

    toast.success(t('skills.manageAgents.success'));
    useSkillDialogStore.getState().closeManageAgents();
    const { useSkillsDataStore } = await import('@/stores/skills-data');
    await useSkillsDataStore.getState().syncSkills(context);
    return { status: 'succeeded', response: result };
  } catch (error) {
    if (isStaleManageAgentError(error)) {
      await openManageAgentChanges(manageAgentsSkill, context, manageAgentsProjectPath);
      return { status: 'stale' };
    }
    console.error('[executeManageAgentChanges] Failed:', error);
    return {
      status: 'failed',
      message: appendCrossStorageFailureGuidance(
        formatWorkflowError(error, t),
        context,
        'manageAgents',
        t,
      ),
    };
  }
}
