import { create } from 'zustand';
import {
  connectEnvironment,
  listEnvironments,
  setWslIntegrationEnabled as setWslIntegrationEnabledApi,
} from '@/hooks/useTauriApi';
import { runBusinessWrite } from '@/workflows/install-session-feedback';
import type {
  AppError,
  EnvironmentDiscoverySnapshot,
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
  wslIntegrationSupported: boolean;
  wslIntegrationEnabled: boolean;
  wslCapabilityRevision: number;
  discover: () => Promise<void>;
  retryDiscovery: () => Promise<void>;
  setWslIntegrationEnabled: (enabled: boolean) => Promise<boolean>;
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
let wslSettingInFlight: Promise<void> | null = null;
let environmentRequestSequence = 0;
let wslSettingSequence = 0;
let connectionRequestSequence = 0;
const activeConnectionRequests = new Map<string, number>();

function authoritativeSnapshotState(snapshot: EnvironmentDiscoverySnapshot) {
  const runtimeByEnvironment = Object.fromEntries(
    snapshot.environments.map((entry) => [environmentKey(entry.environment), entry]),
  );
  return {
    environments: snapshot.environments,
    runtimeByEnvironment,
    discoveryState: snapshot.error ? 'error' as const : 'ready' as const,
    discoveryError: snapshot.error,
    discoveryCompletedAt: Date.now(),
    wslIntegrationSupported: snapshot.wslIntegrationSupported,
    wslIntegrationEnabled: snapshot.wslIntegrationEnabled,
    wslCapabilityRevision: snapshot.wslCapabilityRevision ?? 0,
  };
}

export const useEnvironmentStore = create<EnvironmentStoreState>()((set, get) => {
  const requestDiscovery = (force: boolean) => {
    if (wslSettingInFlight) return wslSettingInFlight;
    if (discoveryInFlight) return discoveryInFlight;

    const { discoveryCompletedAt, environments } = get();
    const coolingDown = discoveryCompletedAt !== null
      && Date.now() < discoveryCompletedAt + DISCOVERY_COOLDOWN_MS;
    if (!force && coolingDown) return Promise.resolve();

    if (environments.length === 0) {
      set({ discoveryState: 'loading', discoveryError: null });
    }

    const sequence = ++environmentRequestSequence;
    const request = (async () => {
      try {
        const snapshot = await listEnvironments();
        set((state) => {
          if (sequence !== environmentRequestSequence) return state;
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
            discoveryState: snapshot.error ? 'error' : 'ready',
            discoveryError: snapshot.error,
            wslIntegrationSupported: snapshot.wslIntegrationSupported,
            wslIntegrationEnabled: snapshot.wslIntegrationEnabled,
            wslCapabilityRevision: snapshot.wslCapabilityRevision
              ?? state.wslCapabilityRevision,
          };
        });
      } catch (error) {
        set((state) => {
          if (sequence !== environmentRequestSequence) return state;
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
            discoveryState: 'error',
            discoveryError: toAppError(error),
          };
        });
        throw error;
      } finally {
        if (sequence === environmentRequestSequence) {
          set({ discoveryCompletedAt: Date.now() });
        }
        discoveryInFlight = null;
      }
    })();
    discoveryInFlight = request;
    return request;
  };

  return {
  environments: [],
  runtimeByEnvironment: {},
  discoveryState: 'idle',
  discoveryError: null,
  discoveryCompletedAt: null,
  wslIntegrationSupported: false,
  wslIntegrationEnabled: false,
  wslCapabilityRevision: 0,

  discover: () => requestDiscovery(false),
  retryDiscovery: () => requestDiscovery(true),

  setWslIntegrationEnabled: (enabled) => {
    const sequence = ++environmentRequestSequence;
    const settingSequence = ++wslSettingSequence;
    const request = (async () => {
      try {
        const outcome = await runBusinessWrite(() => setWslIntegrationEnabledApi(enabled));
        if (outcome.status === 'notRun') return false;
        const snapshot = outcome.value;
        set((state) => {
          if (sequence !== environmentRequestSequence) return state;
          return authoritativeSnapshotState(snapshot);
        });
        return true;
      } finally {
        if (settingSequence === wslSettingSequence) wslSettingInFlight = null;
      }
    })();
    wslSettingInFlight = request.then(
      () => undefined,
      () => undefined,
    );
    return request;
  },

  connect: async (environment) => {
    if (environment.kind === 'host') {
      set((state) => ({
        environments: updateEnvironment(state.environments, environment, {
          status: 'available',
          error: null,
        }),
      }));
      return;
    }
    const key = environmentKey(environment);
    const requestSequence = ++connectionRequestSequence;
    activeConnectionRequests.set(key, requestSequence);
    const startingState = get();
    const startingInfo = startingState.runtimeByEnvironment[key]
      ?? startingState.environments.find((entry) => environmentKey(entry.environment) === key);
    const startingRevision = startingInfo?.revision ?? 0;
    const capabilityRevision = startingState.wslCapabilityRevision;
    set((state) => ({
      environments: updateEnvironment(state.environments, environment, {
        status: 'connecting',
        error: null,
      }),
    }));

    try {
      const connected = await connectEnvironment(environment.distro_name);
      set((state) => {
        if (
          activeConnectionRequests.get(key) !== requestSequence
          || state.wslCapabilityRevision !== capabilityRevision
        ) return state;
        const authoritative = newerEnvironment(state.runtimeByEnvironment[key], connected);
        return {
          environments: updateEnvironment(state.environments, environment, authoritative),
          runtimeByEnvironment: {
            ...state.runtimeByEnvironment,
            [key]: authoritative,
          },
        };
      });
    } catch (error) {
      const appError = toAppError(error);
      set((state) => {
        const current = state.runtimeByEnvironment[key]
          ?? state.environments.find((entry) => environmentKey(entry.environment) === key);
        if (
          activeConnectionRequests.get(key) !== requestSequence
          || state.wslCapabilityRevision !== capabilityRevision
          || !current
          || current.revision > startingRevision
        ) return state;
        const failed = {
          ...current,
          status: 'unavailable',
          error: appError,
        } satisfies EnvironmentInfo;
        return {
          environments: updateEnvironment(state.environments, environment, failed),
          runtimeByEnvironment: { ...state.runtimeByEnvironment, [key]: failed },
        };
      });
      throw error;
    } finally {
      if (activeConnectionRequests.get(key) === requestSequence) {
        activeConnectionRequests.delete(key);
      }
    }
  },

  applyRuntimeEvent: (event) => {
    const key = environmentKey(event.environment);
    set((state) => {
      if (
        event.environment.kind === 'wsl'
        && (!state.wslIntegrationEnabled
          || event.capabilityRevision !== state.wslCapabilityRevision)
      ) return state;
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
      };
    });
  },
  };
});
