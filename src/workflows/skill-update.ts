import { create } from 'zustand';
import type { ActiveMutation, AppError, SkillLocationRef, UpdatePreview, UpdateResponse } from '@/bindings';
import { contextKey } from '@/lib/context';
import { previewUpdate, updateSkill, updateSkillsBatch } from '@/hooks/useTauriApi';
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
  preview: UpdatePreview | null;
  previewError: unknown | null;
  result: UpdateResponse | null;
  executionError: AppError | null;
  confirming: boolean;
  conflictDecisions: Set<string>;
  generation: number;
  open: (context: SkillLocationRef, skillNames: string[], batch?: boolean) => Promise<boolean>;
  setConflictDecision: (entryId: string, overwrite: boolean) => void;
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
  previewError: null,
  result: null,
  executionError: null,
  confirming: false,
  conflictDecisions: new Set<string>(),
};

export const useSkillUpdateWorkflow = create<SkillUpdateWorkflowState>()((set, get) => ({
  ...closedState,
  generation: 0,
  open: async (context, skillNames, batch = skillNames.length > 1) => {
    if (isBusinessWriteBlocked()) return false;
    const generation = get().generation + 1;
    // Capture the operation before awaiting so navigation cannot alter execution intent.
    set({
      phase: 'loadingPreview', context, skillNames: [...skillNames], batch,
      preview: null, previewError: null, result: null, executionError: null,
      confirming: false, conflictDecisions: new Set(), generation,
    });
    try {
      const preview = await previewUpdate({ context, skillNames });
      if (get().generation !== generation) return false;
      set({ phase: 'ready', preview });
      return true;
    } catch (previewError) {
      if (get().generation !== generation) return false;
      set({ phase: 'previewError', previewError });
      return false;
    }
  },
  setConflictDecision: (entryId, overwrite) => set((state) => {
    const conflictDecisions = new Set(state.conflictDecisions);
    if (overwrite) conflictDecisions.add(entryId);
    else conflictDecisions.delete(entryId);
    return { conflictDecisions };
  }),
  confirm: async () => {
    if (isBusinessWriteBlocked()) return;
    const { context, skillNames, preview, batch, conflictDecisions, phase, confirming, executionError } = get();
    if (phase !== 'ready' || confirming || !context || !preview) return;
    const generation = get().generation;
    set({ phase: 'executing', confirming: true });
    try {
      const execution = { request: { context, skillNames }, overwritePrivateEntries: [...conflictDecisions] };
      const acknowledgeRedirect = executionError?.kind === 'directDownloadRedirectConfirmationRequired';
      const outcome = await runBusinessWrite(() => (
        batch
          ? updateSkillsBatch(execution, preview.token, acknowledgeRedirect)
          : updateSkill(execution, preview.token, acknowledgeRedirect)
      ));
      if (get().generation !== generation) return;
      if (outcome.status === 'notRun') {
        set({ phase: 'ready', result: null, executionError: null, confirming: false });
        return;
      }
      const result = outcome.value;
      await useSkillsDataStore.getState().applyUpdateResult(context, result);
      if (get().generation !== generation) return;
      set({ phase: 'result', result, executionError: null, confirming: false });
    } catch (error) {
      if (get().generation !== generation) return;
      const executionError = toAppError(error);
      set({
        phase: executionError.kind === 'directDownloadRedirectConfirmationRequired'
          ? 'ready'
          : 'result',
        result: null,
        executionError,
        confirming: false,
      });
    }
  },
  retryFailed: async () => {
    const { context, result, batch } = get();
    if (!context || !result) return;
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
  close: () => set((state) => ({ ...closedState, generation: state.generation + 1 })),
  reset: () => set((state) => ({ ...closedState, generation: state.generation + 1 })),
}));
