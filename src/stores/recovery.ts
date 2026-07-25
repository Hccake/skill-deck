import { create } from 'zustand';
import { listRecoveryResources } from '@/hooks/useTauriApi';
import { toAppError } from '@/utils/to-app-error';
import type {
  AppError, RecoveryResourceStatus,
} from '@/bindings';

type RecoveryLoadState = 'idle' | 'loading' | 'ready' | 'error';

interface RecoveryState {
  resources: RecoveryResourceStatus[];
  state: RecoveryLoadState;
  error: AppError | null;
  load: () => Promise<void>;
}

let loadGeneration = 0;
let inFlightLoad: Promise<void> | null = null;

export const useRecoveryStore = create<RecoveryState>()((set) => ({
  resources: [],
  state: 'idle',
  error: null,

  load: () => {
    if (inFlightLoad) return inFlightLoad;
    const requestId = ++loadGeneration;
    set({ state: 'loading' });
    const request = listRecoveryResources()
      .then((snapshot) => {
        if (requestId !== loadGeneration) return;
        set({
          resources: snapshot.filter((resource) => resource.state !== 'missing'),
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
}));
