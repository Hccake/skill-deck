import { create } from 'zustand';
import { connectEnvironment, listEnvironments } from '@/hooks/useTauriApi';
import type {
  AppError,
  EnvironmentInfo,
  EnvironmentRef,
  EnvironmentRuntimeEvent,
} from '@/bindings';
import { environmentKey } from '@/lib/context';
import { toAppError } from '@/utils/to-app-error';

export { environmentKey } from '@/lib/context';

export type EnvironmentDiscoveryState = 'idle' | 'loading' | 'ready' | 'error';

interface EnvironmentState {
  environments: EnvironmentInfo[];
  discoveryState: EnvironmentDiscoveryState;
  discoveryError: AppError | null;
  errorsByEnvironment: Record<string, AppError | null>;
  discover: () => Promise<void>;
  connect: (environment: EnvironmentRef) => Promise<void>;
  applyRuntimeEvent: (event: EnvironmentRuntimeEvent) => void;
}

function updateEnvironment(
  environments: EnvironmentInfo[],
  environment: EnvironmentRef,
  patch: Partial<EnvironmentInfo>,
): EnvironmentInfo[] {
  const key = environmentKey(environment);
  return environments.map((entry) => (
    environmentKey(entry.environment) === key ? { ...entry, ...patch } : entry
  ));
}

export const useEnvironmentStore = create<EnvironmentState>()((set) => ({
  environments: [],
  discoveryState: 'idle',
  discoveryError: null,
  errorsByEnvironment: {},

  discover: async () => {
    set({ discoveryState: 'loading', discoveryError: null });
    try {
      const snapshot = await listEnvironments();
      set({
        environments: snapshot.environments,
        discoveryState: snapshot.error ? 'error' : 'ready',
        discoveryError: snapshot.error,
      });
    } catch (error) {
      set({
        discoveryState: 'error',
        discoveryError: toAppError(error),
      });
      throw error;
    }
  },

  connect: async (environment) => {
    const key = environmentKey(environment);
    set((state) => ({
      environments: updateEnvironment(state.environments, environment, {
        status: environment.kind === 'host' ? 'available' : 'connecting',
      }),
      errorsByEnvironment: {
        ...state.errorsByEnvironment,
        [key]: null,
      },
    }));
    if (environment.kind === 'host') return;

    try {
      await connectEnvironment(environment.distro_name);
      set((state) => ({
        environments: updateEnvironment(state.environments, environment, {
          status: 'available',
        }),
      }));
    } catch (error) {
      const appError = toAppError(error);
      set((state) => ({
        environments: updateEnvironment(state.environments, environment, {
          status: 'unavailable',
        }),
        errorsByEnvironment: {
          ...state.errorsByEnvironment,
          [key]: appError,
        },
      }));
      throw error;
    }
  },

  applyRuntimeEvent: (event) => {
    const key = environmentKey(event.environment);
    set((state) => {
      const discovered = state.environments.some(
        (entry) => environmentKey(entry.environment) === key,
      );
      if (!discovered) return state;

      return {
        environments: updateEnvironment(state.environments, event.environment, {
          status: event.status,
        }),
        errorsByEnvironment: {
          ...state.errorsByEnvironment,
          [key]: event.status === 'available' ? null : event.error,
        },
      };
    });
  },
}));
