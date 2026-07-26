import { create } from 'zustand';
import type { ContextRef, EnvironmentRef } from '@/bindings';
import { globalContext, sameContext, sameEnvironment } from '@/lib/context';
import { useEnvironmentStore } from './environment';
import { useProjectStore } from './projects';

interface WorkspaceContextState {
  selectedContext: ContextRef;
  pendingEnvironment: EnvironmentRef | null;
  contextRevision: number;
  switchEnvironment: (environment: EnvironmentRef) => Promise<void>;
  selectGlobal: () => void;
  selectProject: (projectId: string) => void;
}

const HOST: EnvironmentRef = { kind: 'host' };

export const useWorkspaceContextStore = create<WorkspaceContextState>()((set, get) => ({
  selectedContext: globalContext(HOST),
  pendingEnvironment: null,
  contextRevision: 0,

  switchEnvironment: async (environment) => {
    if (get().pendingEnvironment) {
      throw new Error('Environment switch already in progress');
    }

    const target = environment;
    const reconnectingCurrentEnvironment = sameEnvironment(
      get().selectedContext.environment,
      target,
    );
    set({ pendingEnvironment: target });
    try {
      await useEnvironmentStore.getState().connect(target);
      if (!reconnectingCurrentEnvironment) {
        set((state) => ({
          selectedContext: globalContext(target),
          contextRevision: state.contextRevision + 1,
        }));
      }
      void useProjectStore.getState().refresh(target).catch(() => undefined);
    } finally {
      set({ pendingEnvironment: null });
    }
  },

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
}));
