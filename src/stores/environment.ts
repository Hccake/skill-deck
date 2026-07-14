import { create } from 'zustand';
import {
  addEnvironmentProject,
  connectEnvironment,
  listEnvironmentProjects,
  listEnvironments,
  removeEnvironmentProject,
  setEnvironmentProjectCrossStorageWarning,
} from '@/hooks/useTauriApi';
import type { EnvironmentInfo, EnvironmentRef, ProjectBinding } from '@/bindings';
import { isMutationWriteBlocked } from './mutation';

export type EnvironmentDiscoveryState = 'idle' | 'loading' | 'ready' | 'error';

interface EnvironmentState {
  environments: EnvironmentInfo[];
  selectedEnvironment: EnvironmentRef;
  projectsByEnvironment: Record<string, ProjectBinding[]>;
  projectsLoaded: Record<string, boolean>;
  discoveryState: EnvironmentDiscoveryState;
  errors: Record<string, string | null>;

  discoverEnvironments: () => Promise<void>;
  selectEnvironment: (environment: EnvironmentRef) => Promise<void>;
  refreshProjects: (environment?: EnvironmentRef) => Promise<ProjectBinding[]>;
  addProject: (nativePath: string, environment?: EnvironmentRef) => Promise<ProjectBinding[]>;
  removeProject: (projectId: string, environment?: EnvironmentRef) => Promise<ProjectBinding[]>;
  suppressCrossStorageWarning: (
    projectId: string,
    environment?: EnvironmentRef,
  ) => Promise<ProjectBinding[]>;
}

export function environmentKey(environment: EnvironmentRef): string {
  return environment.kind === 'host' ? 'host' : `wsl:${environment.distro_name}`;
}

function updateEnvironment(
  environments: EnvironmentInfo[],
  environment: EnvironmentRef,
  patch: Partial<EnvironmentInfo>,
): EnvironmentInfo[] {
  return environments.map((entry) => (
    environmentKey(entry.environment) === environmentKey(environment)
      ? { ...entry, ...patch }
      : entry
  ));
}

export const useEnvironmentStore = create<EnvironmentState>()((set, get) => ({
  environments: [],
  selectedEnvironment: { kind: 'host' },
  projectsByEnvironment: {},
  projectsLoaded: {},
  discoveryState: 'idle',
  errors: {},

  discoverEnvironments: async () => {
    set({ discoveryState: 'loading' });
    try {
      const environments = await listEnvironments();
      set({ environments, discoveryState: 'ready' });
    } catch (error) {
      set({ discoveryState: 'error' });
      throw error;
    }
  },

  selectEnvironment: async (environment) => {
    set((state) => ({
      selectedEnvironment: environment,
      environments: updateEnvironment(state.environments, environment, {
        status: environment.kind === 'host' ? 'available' : 'connecting',
      }),
      errors: { ...state.errors, [environmentKey(environment)]: null },
    }));
    try {
      if (environment.kind === 'wsl') {
        await connectEnvironment(environment.distro_name);
      }
      await get().refreshProjects(environment);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      set((state) => ({
        environments: updateEnvironment(state.environments, environment, {
          status: 'unavailable',
        }),
        errors: { ...state.errors, [environmentKey(environment)]: message },
      }));
      throw error;
    }
  },

  refreshProjects: async (environment = get().selectedEnvironment) => {
    const key = environmentKey(environment);
    try {
      const projects = await listEnvironmentProjects(environment);
      set((state) => ({
        projectsByEnvironment: { ...state.projectsByEnvironment, [key]: projects },
        projectsLoaded: { ...state.projectsLoaded, [key]: true },
        errors: { ...state.errors, [key]: null },
        environments: updateEnvironment(state.environments, environment, { status: 'available' }),
      }));
      return projects;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      set((state) => ({
        projectsLoaded: { ...state.projectsLoaded, [key]: false },
        errors: { ...state.errors, [key]: message },
      }));
      throw error;
    }
  },

  addProject: async (nativePath, environment = get().selectedEnvironment) => {
    const key = environmentKey(environment);
    if (isMutationWriteBlocked()) return get().projectsByEnvironment[key] ?? [];
    const projects = await addEnvironmentProject(environment, nativePath);
    set((state) => ({
      projectsByEnvironment: { ...state.projectsByEnvironment, [key]: projects },
      projectsLoaded: { ...state.projectsLoaded, [key]: true },
    }));
    return projects;
  },

  removeProject: async (projectId, environment = get().selectedEnvironment) => {
    const key = environmentKey(environment);
    if (isMutationWriteBlocked()) return get().projectsByEnvironment[key] ?? [];
    const projects = await removeEnvironmentProject(environment, projectId);
    set((state) => ({
      projectsByEnvironment: { ...state.projectsByEnvironment, [key]: projects },
      projectsLoaded: { ...state.projectsLoaded, [key]: true },
    }));
    return projects;
  },

  suppressCrossStorageWarning: async (
    projectId,
    environment = get().selectedEnvironment,
  ) => {
    const key = environmentKey(environment);
    if (isMutationWriteBlocked()) return get().projectsByEnvironment[key] ?? [];
    const projects = await setEnvironmentProjectCrossStorageWarning(
      environment,
      projectId,
      true,
    );
    set((state) => ({
      projectsByEnvironment: { ...state.projectsByEnvironment, [key]: projects },
      projectsLoaded: { ...state.projectsLoaded, [key]: true },
    }));
    return projects;
  },
}));
