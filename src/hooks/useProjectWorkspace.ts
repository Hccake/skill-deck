import { useCallback, useMemo, useSyncExternalStore } from 'react';
import type { AppError, EnvironmentRef, ProjectInfo } from '@/bindings';
import { environmentKey, sameEnvironment } from '@/lib/context';
import { projectWorkspace } from '@/stores/projects';
import { useEnvironmentStore } from '@/stores/environment';
import type {
  ProjectWorkspaceCommand,
  ProjectWorkspaceInput,
  ProjectWorkspaceResult,
} from '@/lib/projects/workspace';

export interface ProjectWorkspaceView {
  projects: readonly ProjectInfo[];
  hasCompleteSnapshot: boolean;
  error: AppError | null;
  status: ReturnType<typeof useEnvironmentStore.getState>['environments'][number]['status'] | undefined;
  refresh: () => Promise<ProjectWorkspaceResult>;
  add: (nativePath: string) => Promise<ProjectWorkspaceResult>;
  remove: (projectId: string) => Promise<ProjectWorkspaceResult>;
  setCrossStorageWarning: (
    projectId: string,
    suppressed: boolean,
  ) => Promise<ProjectWorkspaceResult>;
}

export function useProjectWorkspace(
  environment: EnvironmentRef,
): ProjectWorkspaceView {
  const status = useEnvironmentStore((state) => state.environments.find((entry) => (
    sameEnvironment(entry.environment, environment)
  ))?.status);
  const subscribe = useCallback(
    (listener: () => void) => projectWorkspace.subscribe(listener),
    [],
  );
  const getSnapshot = useCallback(
    () => projectWorkspace.getSnapshot(environment),
    [environment],
  );
  const snapshot = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);

  const execute = useCallback(
    (command: ProjectWorkspaceInput): Promise<ProjectWorkspaceResult> => (
      projectWorkspace.execute({ ...command, environment } as ProjectWorkspaceCommand)
    ),
    [environment],
  );
  const refresh = useCallback(() => execute({ kind: 'refresh' }), [execute]);
  const add = useCallback((nativePath: string) => execute({ kind: 'add', nativePath }), [execute]);
  const remove = useCallback((projectId: string) => execute({ kind: 'remove', projectId }), [execute]);
  const setCrossStorageWarning = useCallback(
    (projectId: string, suppressed: boolean) => execute({
      kind: 'setCrossStorageWarning',
      projectId,
      suppressed,
    }),
    [execute],
  );

  return {
    projects: snapshot.projects,
    hasCompleteSnapshot: snapshot.completeness === 'complete',
    error: snapshot.error,
    status,
    refresh,
    add,
    remove,
    setCrossStorageWarning,
  };
}

export function useProjectCatalog(
  environments: readonly EnvironmentRef[],
): Record<string, ProjectInfo[]> {
  const subscribe = useCallback(
    (listener: () => void) => projectWorkspace.subscribe(listener),
    [],
  );
  const getVersionSignature = useCallback(() => environments.map((environment) => {
    const snapshot = projectWorkspace.getSnapshot(environment);
    return `${environmentKey(environment)}:${snapshot.version}`;
  }).join('|'), [environments]);
  const versionSignature = useSyncExternalStore(subscribe, getVersionSignature, () => '');

  return useMemo(() => {
    void versionSignature;
    return Object.fromEntries(environments.map((environment) => [
      environmentKey(environment),
      [...projectWorkspace.getSnapshot(environment).projects],
    ]));
  }, [environments, versionSignature]);
}
