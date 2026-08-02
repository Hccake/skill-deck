import { create } from 'zustand';
import type { AppError, ContextRef, EnvironmentRef } from '@/bindings';
import { globalContext, sameContext, sameEnvironment } from '@/lib/context';
import { toAppError } from '@/utils/to-app-error';
import { useEnvironmentStore } from './environment';
import { useProjectStore } from './projects';

export type WorkspaceTransition =
  | { kind: 'idle' }
  | { kind: 'switchEnvironment'; target: EnvironmentRef }
  | {
    kind: 'wslIntegration';
    phase: 'enabling' | 'switchingHost' | 'disabling';
  };

export type WslIntegrationFailure = {
  stage: 'switchHost' | 'persistSetting' | 'busy';
  error: AppError;
};

export interface WorkspaceContextState {
  selectedContext: ContextRef;
  transition: WorkspaceTransition;
  wslIntegrationFailure: WslIntegrationFailure | null;
  contextRevision: number;
  switchEnvironment: (environment: EnvironmentRef) => Promise<void>;
  changeWslIntegration: (enabled: boolean) => Promise<void>;
  clearWslIntegrationFailure: () => void;
  selectGlobal: () => void;
  selectProject: (projectId: string) => void;
}

const HOST: EnvironmentRef = { kind: 'host' };

export function selectPendingEnvironment(state: WorkspaceContextState): EnvironmentRef | null {
  if (state.transition.kind === 'switchEnvironment') return state.transition.target;
  if (
    state.transition.kind === 'wslIntegration'
    && state.transition.phase === 'switchingHost'
  ) return HOST;
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
    void useProjectStore.getState().refresh(target).catch(() => undefined);
  };

  return {
    selectedContext: globalContext(HOST),
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
      if (get().transition.kind !== 'idle') throw transitionConflict();
      const switchHost = !enabled
        && get().selectedContext.environment.kind === 'wsl';
      set({
        transition: {
          kind: 'wslIntegration',
          phase: switchHost ? 'switchingHost' : enabled ? 'enabling' : 'disabling',
        },
        wslIntegrationFailure: null,
      });
      try {
        if (switchHost) {
          try {
            await connectAndCommit(HOST);
          } catch (error) {
            set({
              wslIntegrationFailure: {
                stage: 'switchHost',
                error: toAppError(error),
              },
            });
            throw error;
          }
          set({ transition: { kind: 'wslIntegration', phase: 'disabling' } });
        }

        try {
          await useEnvironmentStore.getState().setWslIntegrationEnabled(enabled);
        } catch (error) {
          const appError = toAppError(error);
          set({
            wslIntegrationFailure: {
              stage: appError.kind === 'wslIntegrationBusy' ? 'busy' : 'persistSetting',
              error: appError,
            },
          });
          throw error;
        }
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
      const next: ContextRef = {
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
