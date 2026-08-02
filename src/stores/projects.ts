import { create } from 'zustand';
import {
  addEnvironmentProject,
  listEnvironmentProjects,
  removeEnvironmentProject,
  setEnvironmentProjectCrossStorageWarning,
} from '@/hooks/useTauriApi';
import type {
  AddProjectResult,
  AppError,
  EnvironmentRef,
  ProjectInfo,
} from '@/bindings';
import { environmentKey } from '@/lib/context';
import { toAppError } from '@/utils/to-app-error';
import { isBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';

export type ProjectLoadState = 'idle' | 'loading' | 'ready' | 'error';

interface ProjectState {
  projectsByEnvironment: Record<string, ProjectInfo[]>;
  loadStateByEnvironment: Record<string, ProjectLoadState>;
  errorsByEnvironment: Record<string, AppError | null>;
  refresh: (environment: EnvironmentRef) => Promise<ProjectInfo[]>;
  add: (environment: EnvironmentRef, nativePath: string) => Promise<AddProjectResult>;
  remove: (environment: EnvironmentRef, projectId: string) => Promise<ProjectInfo[]>;
  setCrossStorageWarning: (
    environment: EnvironmentRef,
    projectId: string,
    suppressed: boolean,
  ) => Promise<ProjectInfo>;
}

const refreshGenerations = new Map<string, number>();

function nextRefreshGeneration(key: string): number {
  const generation = (refreshGenerations.get(key) ?? 0) + 1;
  refreshGenerations.set(key, generation);
  return generation;
}

function isCurrentRefresh(key: string, generation: number): boolean {
  return refreshGenerations.get(key) === generation;
}

function requireWriteAvailable(): void {
  if (isBusinessWriteBlocked()) {
    throw new Error('Another write operation is already running');
  }
}

function upsertProject(projects: ProjectInfo[], project: ProjectInfo): ProjectInfo[] {
  const existing = projects.findIndex((entry) => entry.binding.id === project.binding.id);
  if (existing < 0) return [...projects, project];
  return projects.map((entry, index) => index === existing ? project : entry);
}

export const useProjectStore = create<ProjectState>()((set) => ({
  projectsByEnvironment: {},
  loadStateByEnvironment: {},
  errorsByEnvironment: {},

  refresh: async (environment) => {
    const key = environmentKey(environment);
    const generation = nextRefreshGeneration(key);
    set((state) => ({
      loadStateByEnvironment: {
        ...state.loadStateByEnvironment,
        [key]: 'loading',
      },
      errorsByEnvironment: {
        ...state.errorsByEnvironment,
        [key]: null,
      },
    }));
    try {
      const projects = await listEnvironmentProjects(environment);
      if (isCurrentRefresh(key, generation)) {
        set((state) => ({
          projectsByEnvironment: {
            ...state.projectsByEnvironment,
            [key]: projects,
          },
          loadStateByEnvironment: {
            ...state.loadStateByEnvironment,
            [key]: 'ready',
          },
        }));
      }
      return projects;
    } catch (error) {
      if (isCurrentRefresh(key, generation)) {
        set((state) => ({
          loadStateByEnvironment: {
            ...state.loadStateByEnvironment,
            [key]: 'error',
          },
          errorsByEnvironment: {
            ...state.errorsByEnvironment,
            [key]: toAppError(error),
          },
        }));
      }
      throw error;
    }
  },

  add: async (environment, nativePath) => {
    requireWriteAvailable();
    const key = environmentKey(environment);
    const result = await addEnvironmentProject(environment, nativePath);
    nextRefreshGeneration(key);
    set((state) => ({
      projectsByEnvironment: {
        ...state.projectsByEnvironment,
        [key]: upsertProject(state.projectsByEnvironment[key] ?? [], result.project),
      },
      loadStateByEnvironment: {
        ...state.loadStateByEnvironment,
        [key]: 'ready',
      },
      errorsByEnvironment: {
        ...state.errorsByEnvironment,
        [key]: null,
      },
    }));
    return result;
  },

  remove: async (environment, projectId) => {
    requireWriteAvailable();
    const key = environmentKey(environment);
    const projects = await removeEnvironmentProject(environment, projectId);
    nextRefreshGeneration(key);
    set((state) => ({
      projectsByEnvironment: {
        ...state.projectsByEnvironment,
        [key]: projects,
      },
      loadStateByEnvironment: {
        ...state.loadStateByEnvironment,
        [key]: 'ready',
      },
      errorsByEnvironment: {
        ...state.errorsByEnvironment,
        [key]: null,
      },
    }));
    return projects;
  },

  setCrossStorageWarning: async (environment, projectId, suppressed) => {
    requireWriteAvailable();
    const key = environmentKey(environment);
    const project = await setEnvironmentProjectCrossStorageWarning(
      environment,
      projectId,
      suppressed,
    );
    nextRefreshGeneration(key);
    set((state) => ({
      projectsByEnvironment: {
        ...state.projectsByEnvironment,
        [key]: upsertProject(state.projectsByEnvironment[key] ?? [], project),
      },
      errorsByEnvironment: {
        ...state.errorsByEnvironment,
        [key]: null,
      },
    }));
    return project;
  },
}));
