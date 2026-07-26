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

interface EnvironmentSchedulerState {
  /** 上次 Discovery 尝试完成的时间，仅供 store 内部调度使用。 */
  discoveryCompletedAt: number | null;
}

type EnvironmentStoreState = EnvironmentState & EnvironmentSchedulerState;

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

const FALLBACK_HOST_ENVIRONMENT: EnvironmentInfo = {
  environment: { kind: 'host' },
  displayName: 'Host',
  status: 'available',
  revision: 0,
  error: null,
};

function withHostFallback(
  environments: EnvironmentInfo[],
  runtimeByEnvironment: Record<string, EnvironmentInfo>,
): EnvironmentInfo[] {
  if (environments.some((entry) => entry.environment.kind === 'host')) return environments;
  return [runtimeByEnvironment.host ?? FALLBACK_HOST_ENVIRONMENT, ...environments];
}

const DISCOVERY_COOLDOWN_MS = 30_000;
let discoveryInFlight: Promise<void> | null = null;
let discoverySequence = 0;

export const useEnvironmentStore = create<EnvironmentStoreState>()((set, get) => ({
  environments: [],
  runtimeByEnvironment: {},
  discoveryState: 'idle',
  discoveryError: null,
  errorsByEnvironment: {},
  discoveryCompletedAt: null,

  discover: () => {
    if (discoveryInFlight) return discoveryInFlight;

    const { discoveryCompletedAt, environments } = get();
    const coolingDown = discoveryCompletedAt !== null
      && Date.now() < discoveryCompletedAt + DISCOVERY_COOLDOWN_MS;
    if (coolingDown) return Promise.resolve();

    if (environments.length === 0) {
      set({ discoveryState: 'loading', discoveryError: null });
    }

    const sequence = ++discoverySequence;
    const request = (async () => {
      try {
        const snapshot = await listEnvironments();
        set((state) => {
          const discovered = snapshot.environments.map((entry) => {
            const key = environmentKey(entry.environment);
            return newerEnvironment(state.runtimeByEnvironment[key], entry);
          });
          const discoveredByKey = Object.fromEntries(
            discovered.map((entry) => [environmentKey(entry.environment), entry]),
          );
          const retainedEnvironments = Array.from(new Map([
              ...state.environments.map((entry) => [environmentKey(entry.environment), entry] as const),
              ...discovered.map((entry) => [environmentKey(entry.environment), entry] as const),
            ]).values());
          const nextEnvironments = snapshot.error
            ? withHostFallback(retainedEnvironments, state.runtimeByEnvironment)
            : discovered;
          const nextByKey = Object.fromEntries(
            nextEnvironments.map((entry) => [environmentKey(entry.environment), entry]),
          );
          const runtimeByEnvironment = snapshot.error
            ? { ...state.runtimeByEnvironment, ...discoveredByKey, ...nextByKey }
            : discoveredByKey;

          return {
            environments: nextEnvironments,
            runtimeByEnvironment,
            errorsByEnvironment: Object.fromEntries(
              nextEnvironments.map((entry) => [environmentKey(entry.environment), entry.error]),
            ),
            discoveryState: snapshot.error ? 'error' : 'ready',
            discoveryError: snapshot.error,
          };
        });
      } catch (error) {
        set((state) => {
          const environments = withHostFallback(
            state.environments,
            state.runtimeByEnvironment,
          );
          const host = environments.find((entry) => entry.environment.kind === 'host');
          return {
            environments,
            runtimeByEnvironment: host
              ? { ...state.runtimeByEnvironment, host }
              : state.runtimeByEnvironment,
            errorsByEnvironment: host
              ? { ...state.errorsByEnvironment, host: host.error }
              : state.errorsByEnvironment,
            discoveryState: 'error',
            discoveryError: toAppError(error),
          };
        });
        throw error;
      } finally {
        set({ discoveryCompletedAt: Date.now() });
        if (discoverySequence === sequence) discoveryInFlight = null;
      }
    })();
    discoveryInFlight = request;
    return request;
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
