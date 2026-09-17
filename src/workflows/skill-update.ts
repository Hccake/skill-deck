import { create } from 'zustand';
import type { ActiveMutation, AppError, SkillLocationRef, PreparedUpdatePreview, UpdateResponse, UpdateSkillPreview } from '@/bindings';
import { contextKey } from '@/lib/context';
import { prepareUpdate, executeUpdate, cancelUpdatePreparation } from '@/hooks/useTauriApi';
import { useSkillsDataStore } from '@/stores/skills-data';
import { toAppError } from '@/utils/to-app-error';
import { isBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import { runBusinessWrite } from './install-session-feedback';

export type SkillUpdatePhase =
  | 'closed'
  | 'loadingPreview'
  | 'previewError'
  | 'ready'
  | 'executing'
  | 'result';

interface SkillUpdateWorkflowState {
  phase: SkillUpdatePhase;
  context: SkillLocationRef | null;
  skillNames: string[];
  batch: boolean;
  preview: PreparedUpdatePreview | null;
  operationId: string | null;
  previewError: unknown | null;
  result: UpdateResponse | null;
  executionError: AppError | null;
  confirming: boolean;
  selectedCopyEntries: Set<string>;
  generation: number;
  open: (context: SkillLocationRef, skillNames: string[], batch?: boolean) => Promise<boolean>;
  setCopySelected: (entryId: string, overwrite: boolean) => void;
  confirm: () => Promise<void>;
  retryFailed: () => Promise<void>;
  acceptMutation: (mutation: ActiveMutation | null) => void;
  close: () => void;
  reset: () => void;
}

const closedState = {
  phase: 'closed' as const,
  context: null,
  skillNames: [],
  batch: false,
  preview: null,
  operationId: null,
  previewError: null,
  result: null,
  executionError: null,
  confirming: false,
  selectedCopyEntries: new Set<string>(),
};

export function canPrepareUpdateAgain(error: AppError | null): boolean {
  return error != null && ['stalePayload', 'payloadSessionExpired', 'staleTarget', 'staleContext', 'staleEnvironment', 'staleRegistry', 'mutationBusy'].includes(error.kind);
}

export function canConfirmSkillUpdate(skill: UpdateSkillPreview, decisions: ReadonlySet<string>): boolean {
  return skill.capability.canRunUpdate
    && skill.blockingReasons.length === 0
    && (skill.targets.some((target) => !target.selectableEntryId || decisions.has(target.selectableEntryId))
      || skill.overwritePrivateEntries.some((entry) => decisions.has(entry.entryId)));
}

export const useSkillUpdateWorkflow = create<SkillUpdateWorkflowState>()((set, get) => ({
  ...closedState,
  generation: 0,
  open: async (context, skillNames, batch = skillNames.length > 1) => {
    if (isBusinessWriteBlocked()) return false;
    const previous = get().operationId;
    if (previous) void cancelUpdatePreparation(previous).catch(() => {});
    const operationId = crypto.randomUUID();
    const generation = get().generation + 1;
    // Capture the operation before awaiting so navigation cannot alter execution intent.
    set({
      phase: 'loadingPreview', context, skillNames: [...skillNames], batch,
      preview: null, previewError: null, result: null, executionError: null,
      confirming: false, selectedCopyEntries: new Set(), generation, operationId,
    });
    try {
      const preview = await prepareUpdate(operationId, { context, skillNames });
      if (get().generation !== generation) {
        void cancelUpdatePreparation(operationId).catch(() => {});
        return false;
      }
      set({ phase: 'ready', preview, selectedCopyEntries: new Set(preview.skills
        .filter((skill) => !preview.blocked.some((issue) => issue.skillName === skill.skillName))
        .flatMap((skill) => skill.targets.flatMap((target) => target.selectableEntryId ? [target.selectableEntryId] : []))) });
      return true;
    } catch (previewError) {
      if (get().generation !== generation) return false;
      set({ phase: 'previewError', previewError });
      return false;
    }
  },
  setCopySelected: (entryId, overwrite) => set((state) => {
    const selectedCopyEntries = new Set(state.selectedCopyEntries);
    if (overwrite) selectedCopyEntries.add(entryId);
    else selectedCopyEntries.delete(entryId);
    return { selectedCopyEntries };
  }),
  confirm: async () => {
    if (isBusinessWriteBlocked()) return;
    const { context, preview, operationId, selectedCopyEntries, phase, confirming } = get();
    if (phase !== 'ready' || confirming || !context || !preview || !operationId) return;
    if (!preview.skills.some((skill) => !preview.blocked.some((issue) => issue.skillName === skill.skillName)
      && canConfirmSkillUpdate(skill, selectedCopyEntries))) return;
    const generation = get().generation;
    set({ phase: 'executing', confirming: true });
    try {
      const outcome = await runBusinessWrite(() => executeUpdate(operationId, [...selectedCopyEntries]));
      if (get().generation !== generation) return;
      if (outcome.status === 'notRun') {
        set({ phase: 'ready', result: null, executionError: null, confirming: false });
        return;
      }
      const result = outcome.value;
      await useSkillsDataStore.getState().applyUpdateResult(context, result);
      if (get().generation !== generation) return;
      set({ phase: 'result', result, executionError: null, confirming: false, operationId: null });
    } catch (error) {
      if (get().generation !== generation) return;
      const executionError = toAppError(error);
      void cancelUpdatePreparation(operationId).catch(() => {});
      set({
        phase: 'result',
        result: null,
        executionError,
        confirming: false,
        operationId: null,
      });
    }
  },
  retryFailed: async () => {
    const { context, result, batch, executionError } = get();
    if (!context) return;
    if (!result) {
      if (canPrepareUpdateAgain(executionError)) await get().open(context, get().skillNames, batch);
      return;
    }
    const skillNames = result.skills
      .filter((skill) => skill.retryable)
      .map((skill) => skill.skillIdentity.skillName);
    if (skillNames.length === 0) return;
    await get().open(context, skillNames, batch || skillNames.length > 1);
  },
  acceptMutation: (mutation) => {
    const { context, phase } = get();
    if (
      !context
      || !mutation
      || mutation.kind !== 'update'
      || mutation.target.kind !== 'skillLocation'
      || contextKey({
        environment: mutation.target.environment,
        scope: mutation.target.scope,
      }) !== contextKey(context)
    ) return;
    if (phase !== 'result' && phase !== 'closed') set({ phase: 'executing' });
  },
  close: () => {
    const { operationId, generation, phase } = get();
    if (phase === 'executing') return;
    if (operationId) void cancelUpdatePreparation(operationId).catch(() => {});
    set({ ...closedState, generation: generation + 1 });
  },
  reset: () => {
    const { operationId, generation } = get();
    if (operationId) void cancelUpdatePreparation(operationId).catch(() => {});
    set({ ...closedState, generation: generation + 1 });
  },
}));
