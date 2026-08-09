import { toast } from 'sonner';
import {
  getManageAgentSelection,
  manageSkillAgents,
  previewManageSkillAgents,
} from '@/hooks/useTauriApi';
import { isBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { t } from '@/stores/skills-utils';
import type {
  AgentSelectionSubmission,
  SkillLocationRef,
  ManageAgentSelectionSnapshot,
  ManageAgentsResponse,
  MutationUnitResult,
  RecoveryAction,
} from '@/bindings';
import { runBusinessWrite } from './install-session-feedback';

const STALE_CODES = new Set([
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
  | { status: 'stale'; snapshot: ManageAgentSelectionSnapshot }
  | { status: 'confirmationRequired' }
  | { status: 'recoveryRequired'; response: ManageAgentsResponse; recovery: RecoveryAction[] }
  | { status: 'failed' };

function recoveryActions(units: MutationUnitResult[]): RecoveryAction[] {
  const seen = new Set<string>();
  return units.flatMap((unit) => {
    const recovery = unit.recovery;
    if (!recovery || seen.has(recovery.resourceId)) return [];
    seen.add(recovery.resourceId);
    return [recovery];
  });
}

function isStaleError(error: unknown): boolean {
  return Boolean(error && typeof error === 'object' && 'kind' in error
    && typeof error.kind === 'string'
    && (STALE_CODES.has(error.kind) || error.kind === 'staleAgentRuntime'));
}

async function reloadManageAgentSelection(
  context: SkillLocationRef,
  skillName: string,
): Promise<ManageAgentsOutcome> {
  try {
    return {
      status: 'stale',
      snapshot: await getManageAgentSelection(context, skillName),
    };
  } catch (error) {
    console.warn('Failed to refresh Agent management selection:', error);
    return { status: 'failed' };
  }
}

export async function executeManageAgentChanges(
  agentSelection: AgentSelectionSubmission,
  confirmEntityDirectories = false,
): Promise<ManageAgentsOutcome> {
  if (isBusinessWriteBlocked()) return { status: 'blocked' };
  const {
    manageAgentsSkill,
    manageAgentsContext,
  } = useSkillDialogStore.getState();
  if (!manageAgentsSkill || !manageAgentsContext) return { status: 'blocked' };

  try {
    const previewOutcome = await previewManageSkillAgents({
      context: manageAgentsContext,
      skillName: manageAgentsSkill.name,
      agentSelection,
    });
    if (previewOutcome.status === 'selectionStale') {
      return { status: 'stale', snapshot: previewOutcome.snapshot };
    }
    const preview = previewOutcome.preview;
    if (preview.confirmation?.removesEntityDirectories && !confirmEntityDirectories) {
      return { status: 'confirmationRequired' };
    }
    const execution = await runBusinessWrite(() => manageSkillAgents({
      token: preview.token,
      context: manageAgentsContext,
      skillName: manageAgentsSkill.name,
      agentSelection,
      confirmEntityDirectories,
      canonicalPayload: preview.canonicalPayload,
    }));
    if (execution.status === 'notRun') return { status: 'blocked' };
    const result = execution.value;
    const failedUnits = result.units.filter((unit) => unit.status !== 'succeeded');
    if (failedUnits.some((unit) => unit.error && STALE_CODES.has(unit.error.code))) {
      return reloadManageAgentSelection(manageAgentsContext, manageAgentsSkill.name);
    }
    const recovery = recoveryActions(result.units);
    if (recovery.length > 0) return { status: 'recoveryRequired', response: result, recovery };
    if (failedUnits.length > 0) return { status: 'failed' };

    toast.success(t('skills.manageAgents.success'));
    useSkillDialogStore.getState().closeManageAgents();
    const { useSkillsDataStore } = await import('@/stores/skills-data');
    await useSkillsDataStore.getState().syncSkills(manageAgentsContext, {
      origin: 'selfMutation',
      mutatedSkillNames: [manageAgentsSkill.name],
    });
    return { status: 'succeeded', response: result };
  } catch (error) {
    if (isStaleError(error)) {
      return reloadManageAgentSelection(manageAgentsContext, manageAgentsSkill.name);
    }
    console.error('[executeManageAgentChanges] Failed:', error);
    return { status: 'failed' };
  }
}
