import { create } from 'zustand';
import type {
  EnvironmentRef,
  LibraryId,
  LibraryUpdateContinuation,
  LibraryUpdatePreviewToken,
  LibraryUpdateResponse,
  LibraryUpdateSkillStatus,
  SkillUpdateInfo,
  UpdateLibrarySkillsRequest,
} from '@/bindings';
import {
  checkLibrarySkillUpdates,
  previewLibrarySkillUpdates,
  updateLibrarySkills,
} from '@/hooks/useTauriApi';
import { environmentKey } from '@/lib/context';
import type { LibraryUpdatePhase } from '@/lib/libraries/update-progress';

interface PendingLibraryUpdate {
  request: UpdateLibrarySkillsRequest;
  token: LibraryUpdatePreviewToken;
  continuation: LibraryUpdateContinuation | null;
  redirectedDownloadHosts: string[];
}

interface LibraryUpdateWorkflowState {
  phase: LibraryUpdatePhase;
  environment: EnvironmentRef | null;
  libraryId: LibraryId | null;
  checks: Record<string, SkillUpdateInfo>;
  /** 上一批更新中每个成员的提交结果，用于卡片显示完成或失败。 */
  lastResults: Record<string, LibraryUpdateSkillStatus>;
  hasError: boolean;
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
    set({ phase: 'checking', hasError: false });
    try {
      const response = await checkLibrarySkillUpdates(environment, libraryId);
      if (get().generation !== generation) return;
      set((state) => ({
        phase: 'idle',
        checks: Object.fromEntries(response.skills.map((check) => {
          const previous = state.checks[check.name];
          return [
            check.name,
            check.status === 'cannotCheck' && previous && previous.status !== 'cannotCheck'
              ? previous
              : check,
          ];
        })),
        hasError: response.outcome !== 'completed',
      }));
    } catch {
      if (get().generation === generation) set({ phase: 'idle', hasError: true });
    }
  },
  prepare: async (skillNames) => {
    const { environment, libraryId, generation, phase } = get();
    if (!environment || !libraryId || skillNames.length === 0 || phase !== 'idle') return;
    const request = { environment, libraryId, skillNames: [...skillNames] };
    set({ phase: 'preparing', hasError: false, lastResults: {} });
    try {
      const preview = await previewLibrarySkillUpdates(request);
      if (get().generation !== generation) return;
      set({
        phase: 'ready',
        pending: {
          request,
          token: preview.token,
          continuation: null,
          redirectedDownloadHosts: [],
        },
      });
    } catch {
      if (get().generation === generation) set({ phase: 'idle', hasError: true });
    }
  },
  confirm: async () => {
    const { pending, generation, phase } = get();
    if (!pending || phase !== 'ready') return null;
    set({ phase: 'executing', hasError: false });
    try {
      const outcome = await updateLibrarySkills({
        request: pending.request,
        expectedToken: pending.token,
        continuation: pending.continuation,
        riskConfirmation: pending.redirectedDownloadHosts.length > 0
          ? { redirectedDownloadHosts: pending.redirectedDownloadHosts }
          : null,
      });
      if (get().generation !== generation) return null;
      if (outcome.status === 'confirmationRequired') {
        set({
          phase: 'ready',
          pending: {
            request: pending.request,
            token: outcome.token,
            continuation: outcome.continuation,
            redirectedDownloadHosts: outcome.redirectedDownloadHosts,
          },
        });
        return null;
      }
      const hasError = outcome.response.results.some((result) => result.status !== 'succeeded');
      set({
        phase: 'idle',
        checks: {},
        pending: null,
        hasError,
        lastResults: Object.fromEntries(
          outcome.response.results.map((result) => [result.skillName, result.status]),
        ),
      });
      return outcome.response;
    } catch {
      if (get().generation === generation) set({ phase: 'ready', hasError: true });
      return null;
    }
  },
  cancel: () => set((state) => ({
    phase: 'idle',
    pending: null,
    generation: state.generation + 1,
  })),
  reset: () => set((state) => ({ ...initialState, generation: state.generation + 1 })),
}));
