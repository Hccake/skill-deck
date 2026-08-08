import { create } from 'zustand';
import type { AppError, SkillLocationRef, EnvironmentRef } from '@/bindings';
import { globalContext, sameContext, sameEnvironment } from '@/lib/context';
import { toAppError } from '@/utils/to-app-error';
import { useEnvironmentStore } from './environment';
import { projectWorkspace, registerProjectCatalogObserver } from './projects';

export type WorkspaceTransition =
  | { kind: 'idle' }
  | { kind: 'switchEnvironment'; target: EnvironmentRef }
  | {
    kind: 'wslIntegration';
    phase: 'enabling' | 'switchingNative' | 'disabling';
  };

export type WslIntegrationFailure = {
  stage: 'switchHost' | 'persistSetting' | 'busy';
  error: AppError;
};

export type WslIntegrationChangeOutcome =
  | { status: 'succeeded' }
  | { status: 'notRun' }
  | { status: 'failed'; failure: WslIntegrationFailure };

export interface WorkspaceContextState {
  selectedContext: SkillLocationRef;
  transition: WorkspaceTransition;
  wslIntegrationFailure: WslIntegrationFailure | null;
  contextRevision: number;
  switchEnvironment: (environment: EnvironmentRef) => Promise<void>;
  changeWslIntegration: (enabled: boolean) => Promise<WslIntegrationChangeOutcome>;
  clearWslIntegrationFailure: () => void;
  selectGlobal: () => void;
  selectProject: (projectId: string) => void;
}

const NATIVE: EnvironmentRef = { kind: 'native' };

export function selectPendingEnvironment(state: WorkspaceContextState): EnvironmentRef | null {
  if (state.transition.kind === 'switchEnvironment') return state.transition.target;
  if (
    state.transition.kind === 'wslIntegration'
    && state.transition.phase === 'switchingNative'
  ) return NATIVE;
  return null;
}

export function selectWorkspaceTransitionActive(state: WorkspaceContextState): boolean {
  return state.transition.kind !== 'idle';
}

function transitionConflict() {
  return new Error('Workspace transition already in progress');
}

export const useWorkspaceContextStore = create<WorkspaceContextState>()((set, get) => {
  const connectAndCommit = async (target: EnvironmentRef) => {
    const reconnectingCurrentEnvironment = sameEnvironment(
      get().selectedContext.environment,
      target,
    );
    await useEnvironmentStore.getState().connect(target);
    if (!reconnectingCurrentEnvironment) {
      set((state) => ({
        selectedContext: globalContext(target),
        contextRevision: state.contextRevision + 1,
      }));
    }
    void projectWorkspace.execute({ kind: 'refresh', environment: target, reason: 'reconnect' });
  };

  return {
    selectedContext: globalContext(NATIVE),
    transition: { kind: 'idle' },
    wslIntegrationFailure: null,
    contextRevision: 0,

    switchEnvironment: async (environment) => {
      if (get().transition.kind !== 'idle') throw transitionConflict();
      set({ transition: { kind: 'switchEnvironment', target: environment } });
      try {
        await connectAndCommit(environment);
      } finally {
        set({ transition: { kind: 'idle' } });
      }
    },

    changeWslIntegration: async (enabled) => {
      if (get().transition.kind !== 'idle') {
        const failure: WslIntegrationFailure = {
          stage: 'busy',
          error: { kind: 'mutationBusy' },
        };
        set({ wslIntegrationFailure: failure });
        return { status: 'failed', failure };
      }
      const switchHost = !enabled
        && get().selectedContext.environment.kind === 'wsl';
      set({
        transition: {
          kind: 'wslIntegration',
          phase: switchHost ? 'switchingNative' : enabled ? 'enabling' : 'disabling',
        },
        wslIntegrationFailure: null,
      });
      try {
        if (switchHost) {
          try {
            await connectAndCommit(NATIVE);
          } catch (error) {
            const failure: WslIntegrationFailure = {
              stage: 'switchHost',
              error: toAppError(error),
            };
            set({ wslIntegrationFailure: failure });
            return { status: 'failed', failure };
          }
          set({ transition: { kind: 'wslIntegration', phase: 'disabling' } });
        }

        try {
          const changed = await useEnvironmentStore.getState().setWslIntegrationEnabled(enabled);
          if (!changed) return { status: 'notRun' };
        } catch (error) {
          const appError = toAppError(error);
          const failure: WslIntegrationFailure = {
            stage: appError.kind === 'wslIntegrationBusy' ? 'busy' : 'persistSetting',
            error: appError,
          };
          set({ wslIntegrationFailure: failure });
          return { status: 'failed', failure };
        }
        return { status: 'succeeded' };
      } finally {
        set({ transition: { kind: 'idle' } });
      }
    },

    clearWslIntegrationFailure: () => set({ wslIntegrationFailure: null }),

    selectGlobal: () => {
      const next = globalContext(get().selectedContext.environment);
      if (sameContext(get().selectedContext, next)) return;
      set((state) => ({
        selectedContext: next,
        contextRevision: state.contextRevision + 1,
      }));
    },

    selectProject: (projectId) => {
      const next: SkillLocationRef = {
        environment: get().selectedContext.environment,
        scope: { scope: 'project', project_id: projectId },
      };
      if (sameContext(get().selectedContext, next)) return;
      set((state) => ({
        selectedContext: next,
        contextRevision: state.contextRevision + 1,
      }));
    },
  };
});

registerProjectCatalogObserver({
  captureContext: () => {
    const state = useWorkspaceContextStore.getState();
    return {
      context: state.selectedContext,
      revision: state.contextRevision,
    };
  },
  onCompleteSnapshot: ({ environment, projects, expectedContext }) => {
    const state = useWorkspaceContextStore.getState();
    if (
      state.contextRevision !== expectedContext.revision
      || !sameContext(state.selectedContext, expectedContext.context)
      || !sameEnvironment(state.selectedContext.environment, environment)
      || state.selectedContext.scope.scope !== 'project'
    ) return;
    const projectId = state.selectedContext.scope.project_id;
    if (projects.some((project) => project.binding.id === projectId)) return;
    state.selectGlobal();
  },
});
