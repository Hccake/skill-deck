import { create } from 'zustand';
import { listRecoveryResources, retryRuntimeMaintenance } from '@/hooks/useTauriApi';
import { toAppError } from '@/utils/to-app-error';
import { environmentKey } from '@/lib/context';
import type {
  AppError, EnvironmentRef, RecoveryResourceStatus, RuntimeMaintenanceStatus,
} from '@/bindings';

type RecoveryLoadState = 'idle' | 'loading' | 'ready' | 'error';

interface RecoveryState {
  resources: RecoveryResourceStatus[];
  maintenance: RuntimeMaintenanceStatus[];
  state: RecoveryLoadState;
  error: AppError | null;
  load: () => Promise<void>;
  applyMaintenance: (status: RuntimeMaintenanceStatus) => void;
  retryMaintenance: (environment: EnvironmentRef) => Promise<void>;
}

let loadGeneration = 0;
let inFlightLoad: Promise<void> | null = null;

export const useRecoveryStore = create<RecoveryState>()((set) => ({
  resources: [],
  maintenance: [],
  state: 'idle',
  error: null,

  load: () => {
    if (inFlightLoad) return inFlightLoad;
    const requestId = ++loadGeneration;
    set({ state: 'loading', error: null });
    const request = listRecoveryResources()
      .then((snapshot) => {
        if (requestId !== loadGeneration) return;
        const normalized = Array.isArray(snapshot)
          ? { resources: snapshot, maintenance: [] }
          : snapshot;
        set({
          resources: normalized.resources.filter((resource) => resource.state !== 'missing'),
          maintenance: normalized.maintenance,
          state: 'ready',
          error: null,
        });
      })
      .catch((error) => {
        if (requestId !== loadGeneration) return;
        set({ state: 'error', error: toAppError(error) });
      })
      .finally(() => {
        if (inFlightLoad === request) inFlightLoad = null;
      });
    inFlightLoad = request;
    return request;
  },

  applyMaintenance: (status) => set((state) => {
    const key = environmentKey(status.environment);
    const maintenance = state.maintenance.filter((item) => environmentKey(item.environment) !== key);
    return { maintenance: [...maintenance, status] };
  }),

  retryMaintenance: async (environment) => {
    const status = await retryRuntimeMaintenance(environment);
    set((state) => {
      const key = environmentKey(status.environment);
      const maintenance = state.maintenance.filter((item) => environmentKey(item.environment) !== key);
      return { maintenance: [...maintenance, status] };
    });
    await useRecoveryStore.getState().load();
  },
}));
