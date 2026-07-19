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
  runtimeByEnvironment: Record<string, EnvironmentInfo>;
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

function newerEnvironment(current: EnvironmentInfo | undefined, candidate: EnvironmentInfo) {
  return !current || candidate.revision > current.revision ? candidate : current;
}

function runtimeInfo(event: EnvironmentRuntimeEvent, previous?: EnvironmentInfo): EnvironmentInfo {
  return {
    environment: event.environment,
    displayName: previous?.displayName
      ?? (event.environment.kind === 'host' ? 'Host' : event.environment.distro_name),
    status: event.status,
    revision: event.revision,
    error: event.error,
  };
}

let discoveryGeneration = 0;

export const useEnvironmentStore = create<EnvironmentState>()((set) => ({
  environments: [],
  runtimeByEnvironment: {},
  discoveryState: 'idle',
  discoveryError: null,
  errorsByEnvironment: {},

  discover: async () => {
    const requestId = ++discoveryGeneration;
    set({ discoveryState: 'loading', discoveryError: null });
    try {
      const snapshot = await listEnvironments();
      if (requestId !== discoveryGeneration) return;
      set((state) => {
        const runtimeByEnvironment = { ...state.runtimeByEnvironment };
        const environments = snapshot.environments.map((entry) => {
          const key = environmentKey(entry.environment);
          const merged = newerEnvironment(runtimeByEnvironment[key], entry);
          runtimeByEnvironment[key] = merged;
          return merged;
        });
        return {
          environments,
          runtimeByEnvironment,
          errorsByEnvironment: Object.fromEntries(
            environments.map((entry) => [environmentKey(entry.environment), entry.error]),
          ),
          discoveryState: snapshot.error ? 'error' : 'ready',
          discoveryError: snapshot.error,
        };
      });
    } catch (error) {
      if (requestId !== discoveryGeneration) return;
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
        error: null,
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
          error: null,
        }),
      }));
    } catch (error) {
      const appError = toAppError(error);
      set((state) => ({
        environments: updateEnvironment(state.environments, environment, {
          status: 'unavailable',
          error: appError,
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
      const current = state.runtimeByEnvironment[key]
        ?? state.environments.find((entry) => environmentKey(entry.environment) === key);
      if (current && event.revision <= current.revision) return state;
      const next = runtimeInfo(event, current);
      const discovered = state.environments.some((entry) => (
        environmentKey(entry.environment) === key
      ));

      return {
        runtimeByEnvironment: { ...state.runtimeByEnvironment, [key]: next },
        environments: discovered
          ? updateEnvironment(state.environments, event.environment, next)
          : state.environments,
        errorsByEnvironment: {
          ...state.errorsByEnvironment,
          [key]: event.error,
        },
      };
    });
  },
}));
