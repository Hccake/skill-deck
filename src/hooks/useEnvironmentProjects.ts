import { useCallback, useEffect } from 'react';
import { useEnvironmentStore } from '@/stores/environment';
import { useProjectStore, type ProjectLoadState } from '@/stores/projects';
import { environmentKey, sameEnvironment } from '@/lib/context';
import type { AppError, EnvironmentRef, ProjectInfo } from '@/bindings';

const EMPTY_PROJECTS: ProjectInfo[] = [];

interface UseEnvironmentProjectsOptions {
  autoRefresh?: boolean;
  transitionActive?: boolean;
}

interface EnvironmentProjectsSnapshot {
  projects: ProjectInfo[];
  loadState: ProjectLoadState;
  error: AppError | null | undefined;
  status: ReturnType<typeof useEnvironmentStore.getState>['environments'][number]['status'] | undefined;
  refresh: () => Promise<ProjectInfo[]>;
}

export function useEnvironmentProjects(
  environment: EnvironmentRef,
  {
    autoRefresh = true,
    transitionActive = false,
  }: UseEnvironmentProjectsOptions = {},
): EnvironmentProjectsSnapshot {
  const environments = useEnvironmentStore((state) => state.environments);
  const projectsByEnvironment = useProjectStore((state) => state.projectsByEnvironment);
  const loadStateByEnvironment = useProjectStore((state) => state.loadStateByEnvironment);
  const errorsByEnvironment = useProjectStore((state) => state.errorsByEnvironment);
  const refreshProjects = useProjectStore((state) => state.refresh);
  const key = environmentKey(environment);
  const projects = projectsByEnvironment[key] ?? EMPTY_PROJECTS;
  const loadState = loadStateByEnvironment[key] ?? 'idle';
  const error = errorsByEnvironment[key];
  const status = environments.find((entry) => sameEnvironment(entry.environment, environment))?.status;
  const refresh = useCallback(() => refreshProjects(environment), [environment, refreshProjects]);

  useEffect(() => {
    if (!autoRefresh || loadState !== 'idle' || transitionActive || status !== 'available') return;
    void refresh().catch(() => undefined);
  }, [autoRefresh, loadState, refresh, status, transitionActive]);

  return {
    projects,
    loadState,
    error,
    status,
    refresh,
  };
}
