import { create } from 'zustand';
import { toast } from 'sonner';
import i18n from '@/i18n';
import { updateCheckFailureKey } from '@/lib/skill-status-presentation';
import type {
  EnvironmentRef,
  LibraryId,
  AppError,
  PreparedLibraryUpdatePreview,
  LibraryUpdateResponse,
  LibraryUpdateSkillResult,
  SkillUpdateInfo,
  UpdateLibrarySkillsRequest,
} from '@/bindings';
import {
  checkLibrarySkillUpdates,
  prepareLibrarySkillUpdates,
  cancelUpdatePreparation,
  updateLibrarySkills,
} from '@/hooks/useTauriApi';
import { environmentKey } from '@/lib/context';
import type { LibraryUpdatePhase } from '@/lib/libraries/update-progress';
import { toAppError } from '@/utils/to-app-error';

interface PendingLibraryUpdate extends PreparedLibraryUpdatePreview {
  request: UpdateLibrarySkillsRequest;
  operationId: string;
}

interface LibraryUpdateWorkflowState {
  phase: LibraryUpdatePhase;
  environment: EnvironmentRef | null;
  libraryId: LibraryId | null;
  checks: Record<string, SkillUpdateInfo>;
  /** 上一批更新中每个成员的提交结果，用于卡片显示完成或失败。 */
  lastResults: Record<string, LibraryUpdateSkillResult>;
  hasError: boolean;
  error: AppError | null;
  pending: PendingLibraryUpdate | null;
  generation: number;
  activate: (environment: EnvironmentRef, libraryId: LibraryId | null) => void;
  check: () => Promise<void>;
  prepare: (skillNames: string[]) => Promise<void>;
  confirm: () => Promise<LibraryUpdateResponse | null>;
  cancel: () => void;
  reset: () => void;
}

const initialState = {
  phase: 'idle' as const,
  environment: null,
  libraryId: null,
  checks: {},
  lastResults: {},
  hasError: false,
  error: null,
  pending: null,
};

export const useLibraryUpdateWorkflow = create<LibraryUpdateWorkflowState>()((set, get) => ({
  ...initialState,
  generation: 0,
  activate: (environment, libraryId) => {
    const current = get();
    if (
      current.environment
      && environmentKey(current.environment) === environmentKey(environment)
      && current.libraryId === libraryId
    ) return;
    if (current.pending) void cancelUpdatePreparation(current.pending.operationId).catch(() => {});
    set({
      ...initialState,
      environment,
      libraryId,
      generation: current.generation + 1,
    });
  },
  check: async () => {
    const { environment, libraryId, generation, phase } = get();
    if (!environment || !libraryId || phase !== 'idle') return;
    set({ phase: 'checking', hasError: false, error: null });
    try {
      const response = await checkLibrarySkillUpdates(environment, libraryId);
      if (get().generation !== generation) return;
      set((state) => ({
        phase: 'idle',
        error: null,
        checks: Object.fromEntries(response.skills.map((check) => {
          const previous = state.checks[check.name];
          return [
            check.name,
            check.status === 'cannotCheck' && previous && previous.status !== 'cannotCheck' && previous.comparisonFingerprint === check.comparisonFingerprint
              ? { ...check, hasUpdate: previous.hasUpdate, status: previous.status, reason: previous.reason }
              : check,
          ];
        })),
        hasError: response.skills.some((check) => check.status === 'cannotCheck' && check.reason !== 'missingRemoteHash') || response.sources.some((source) => source.error != null || source.lastAttempt?.failure != null),
      }));
      if (get().hasError) toast.error(i18n.t(updateCheckFailureKey(response)));
    } catch (error) {
      if (get().generation === generation) {
        const appError = toAppError(error);
        set({ phase: 'idle', hasError: true, error: appError });
        toast.error(i18n.t(updateCheckFailureKey(appError)));
      }
    }
  },
  prepare: async (skillNames) => {
    const { environment, libraryId, generation, phase } = get();
    if (!environment || !libraryId || skillNames.length === 0 || phase !== 'idle') return;
    const request = { environment, libraryId, skillNames: [...skillNames] };
    const operationId = crypto.randomUUID();
    set((state) => ({ phase: 'preparing', hasError: false, error: null, lastResults: Object.fromEntries(Object.entries(state.lastResults).filter(([name]) => !skillNames.includes(name))), pending: { request, operationId, skillNames: [], blocked: [], redirectedDownloadHosts: [] } }));
    try {
      const preview = await prepareLibrarySkillUpdates(operationId, request);
      if (get().generation !== generation) {
        void cancelUpdatePreparation(operationId).catch(() => {});
        return;
      }
      set({
        phase: 'ready',
        pending: {
          request,
          operationId,
          ...preview,
        },
      });
    } catch (error) {
      void cancelUpdatePreparation(operationId).catch(() => {});
      if (get().generation === generation) set({ phase: 'idle', pending: null, hasError: true, error: toAppError(error) });
    }
  },
  confirm: async () => {
    const { pending, generation, phase } = get();
    if (!pending || phase !== 'ready' || pending.skillNames.length === 0) return null;
    set({ phase: 'executing', hasError: false });
    try {
      const response = await updateLibrarySkills(pending.operationId);
      if (get().generation !== generation) return null;
      const hasError = response.results.some((result) => result.status !== 'succeeded');
      set((state) => ({
        phase: 'idle',
        checks: Object.fromEntries(Object.entries(state.checks).filter(([name]) => !response.results.some((result) => result.skillName === name && result.status === 'succeeded'))),
        pending: null,
        hasError,
        lastResults: { ...state.lastResults, ...Object.fromEntries(
          response.results.map((result) => [result.skillName, result]),
        ) },
      }));
      return response;
    } catch (error) {
      void cancelUpdatePreparation(pending.operationId).catch(() => {});
      if (get().generation === generation) set({ phase: 'idle', pending: null, hasError: true, error: toAppError(error) });
      return null;
    }
  },
  cancel: () => {
    const { pending, phase, generation } = get();
    if (phase === 'executing') return;
    if (pending) void cancelUpdatePreparation(pending.operationId).catch(() => {});
    set({ phase: 'idle', pending: null, generation: generation + 1 });
  },
  reset: () => {
    const { pending, generation } = get();
    if (pending) void cancelUpdatePreparation(pending.operationId).catch(() => {});
    set({ ...initialState, generation: generation + 1 });
  },
}));
