import type {
  AddProjectResult,
  AppError,
  ContextRef,
  EnvironmentRef,
  ProjectInfo,
} from '@/bindings';
import { environmentKey } from '@/lib/context';
import { toAppError } from '@/utils/to-app-error';

export type ProjectWorkspacePhase = 'idle' | 'loading' | 'ready' | 'error';
export type ProjectCollectionCompleteness = 'partial' | 'complete';
export type ProjectRefreshReason = 'manual' | 'reconnect' | 'focus';

const PROJECT_CATALOG_FRESHNESS_MS = 5 * 60 * 1_000;

export interface ProjectWorkspaceSnapshot {
  environment: EnvironmentRef;
  phase: ProjectWorkspacePhase;
  projects: readonly ProjectInfo[];
  error: AppError | null;
  completeness: ProjectCollectionCompleteness;
  environmentRevision: number;
  lastAttemptAt: number | null;
  lastSuccessAt: number | null;
  freshUntil: number | null;
  version: number;
}

export type ProjectWorkspaceCommand =
  | { kind: 'ensureLoaded'; environment: EnvironmentRef }
  | { kind: 'refresh'; environment: EnvironmentRef; reason?: ProjectRefreshReason }
  | { kind: 'prepareCopyTarget'; environment: EnvironmentRef }
  | { kind: 'add'; environment: EnvironmentRef; nativePath: string }
  | {
      kind: 'remove';
      environment: EnvironmentRef;
      projectId: string;
      expectedContext?: ProjectContextCapture;
    }
  | {
      kind: 'setCrossStorageWarning';
      environment: EnvironmentRef;
      projectId: string;
      suppressed: boolean;
    };

export type ProjectWorkspaceValue = AddProjectResult | ProjectInfo | readonly ProjectInfo[];

export type ProjectWorkspaceResult =
  | {
      status: 'succeeded';
      snapshot: ProjectWorkspaceSnapshot;
      value: ProjectWorkspaceValue;
    }
  | { status: 'notRun'; reason: 'catalogNotReady' | 'writeBlocked' }
  | {
      status: 'failed';
      failureSource: 'environment' | 'catalog' | 'command';
      error: AppError;
      snapshot: ProjectWorkspaceSnapshot;
    };

export interface ProjectBackend {
  list(environment: EnvironmentRef): Promise<ProjectInfo[]>;
  add(environment: EnvironmentRef, nativePath: string): Promise<AddProjectResult>;
  remove(environment: EnvironmentRef, projectId: string): Promise<ProjectInfo[]>;
  setCrossStorageWarning(
    environment: EnvironmentRef,
    projectId: string,
    suppressed: boolean,
  ): Promise<ProjectInfo>;
}

export interface ProjectEnvironmentAccess {
  isAvailable(environment: EnvironmentRef): boolean;
  revision(environment: EnvironmentRef): number;
  ensureAvailable(environment: EnvironmentRef): Promise<void>;
}

export interface ProjectContextCapture {
  context: ContextRef;
  revision: number;
}

export interface ProjectCatalogCompletion {
  environment: EnvironmentRef;
  projects: readonly ProjectInfo[];
  expectedContext: ProjectContextCapture;
}

export interface ProjectCatalogObserver {
  captureContext(): ProjectContextCapture;
  onCompleteSnapshot(completion: ProjectCatalogCompletion): void;
}

export type ProjectWriteOutcome<T> =
  | { status: 'succeeded'; value: T }
  | { status: 'notRun' };

export interface ProjectWriteAccess {
  run<T>(operation: () => Promise<T>): Promise<ProjectWriteOutcome<T>>;
}

export interface ProjectWorkspaceDependencies {
  backend: ProjectBackend;
  environment: ProjectEnvironmentAccess;
  catalogObserver: ProjectCatalogObserver;
  write: ProjectWriteAccess;
  now?: () => number;
}

export interface ProjectWorkspace {
  getSnapshot(environment: EnvironmentRef): ProjectWorkspaceSnapshot;
  subscribe(listener: (snapshot: ProjectWorkspaceSnapshot) => void): () => void;
  execute(command: ProjectWorkspaceCommand): Promise<ProjectWorkspaceResult>;
}

export type ProjectWorkspaceInput = ProjectWorkspaceCommand extends infer Command
  ? Command extends { environment: EnvironmentRef }
    ? Omit<Command, 'environment'>
    : never
  : never;

function initialSnapshot(
  environment: EnvironmentRef,
  environmentRevision: number,
): ProjectWorkspaceSnapshot {
  return {
    environment,
    phase: 'idle',
    projects: [],
    error: null,
    completeness: 'partial',
    environmentRevision,
    lastAttemptAt: null,
    lastSuccessAt: null,
    freshUntil: null,
    version: 0,
  };
}

function upsertProject(projects: readonly ProjectInfo[], project: ProjectInfo): ProjectInfo[] {
  const existing = projects.findIndex((entry) => entry.binding.id === project.binding.id);
  if (existing < 0) return [...projects, project];
  return projects.map((entry, index) => index === existing ? project : entry);
}

export function createProjectWorkspace(
  dependencies: ProjectWorkspaceDependencies,
): ProjectWorkspace {
  const now = dependencies.now ?? Date.now;
  const snapshots = new Map<string, ProjectWorkspaceSnapshot>();
  const requestGenerations = new Map<string, number>();
  const inFlightReads = new Map<string, {
    environmentRevision: number;
    request: Promise<ProjectWorkspaceResult>;
  }>();
  const listeners = new Set<(snapshot: ProjectWorkspaceSnapshot) => void>();

  const getSnapshot = (environment: EnvironmentRef) => {
    const key = environmentKey(environment);
    const existing = snapshots.get(key);
    if (existing) return existing;
    const snapshot = initialSnapshot(environment, dependencies.environment.revision(environment));
    snapshots.set(key, snapshot);
    return snapshot;
  };

  const commit = (
    environment: EnvironmentRef,
    update: (current: ProjectWorkspaceSnapshot) => Omit<ProjectWorkspaceSnapshot, 'version'>,
  ) => {
    const key = environmentKey(environment);
    const current = getSnapshot(environment);
    const next = { ...update(current), version: current.version + 1 };
    snapshots.set(key, next);
    listeners.forEach((listener) => listener(next));
    return next;
  };

  const invalidateRequests = (environment: EnvironmentRef) => {
    const key = environmentKey(environment);
    const generation = (requestGenerations.get(key) ?? 0) + 1;
    requestGenerations.set(key, generation);
    return generation;
  };

  const publishCompleteSnapshot = (
    expected: ProjectContextCapture,
    environment: EnvironmentRef,
    projects: readonly ProjectInfo[],
  ) => {
    dependencies.catalogObserver.onCompleteSnapshot({
      environment,
      projects,
      expectedContext: expected,
    });
  };

  const failed = (
    environment: EnvironmentRef,
    error: unknown,
    failureSource: 'environment' | 'catalog',
  ): ProjectWorkspaceResult => {
    const appError = toAppError(error);
    const snapshot = commit(environment, (current) => ({
      ...current,
      phase: 'error',
      error: appError,
    }));
    return { status: 'failed', failureSource, error: appError, snapshot };
  };

  const commandFailed = (environment: EnvironmentRef, error: unknown): ProjectWorkspaceResult => {
    const appError = toAppError(error);
    return {
      status: 'failed',
      failureSource: 'command',
      error: appError,
      snapshot: getSnapshot(environment),
    };
  };

  const readProjects = async (
    environment: EnvironmentRef,
    environmentRevision: number,
  ): Promise<ProjectWorkspaceResult> => {
    if (!dependencies.environment.isAvailable(environment)) {
      return failed(environment, {
        kind: 'environmentUnavailable',
        data: { environment, message: 'Environment is unavailable' },
      } satisfies AppError, 'environment');
    }
    const key = environmentKey(environment);
    const generation = invalidateRequests(environment);
    commit(environment, (current) => ({
      ...current,
      phase: 'loading',
      error: null,
      environmentRevision,
      lastAttemptAt: now(),
    }));
    try {
      const projects = await dependencies.backend.list(environment);
      if (
        requestGenerations.get(key) !== generation
        || dependencies.environment.revision(environment) !== environmentRevision
      ) {
        const snapshot = getSnapshot(environment);
        return { status: 'succeeded', snapshot, value: snapshot.projects };
      }
      const completedAt = now();
      const snapshot = commit(environment, (current) => ({
        ...current,
        phase: 'ready',
        projects,
        error: null,
        completeness: 'complete',
        lastSuccessAt: completedAt,
        freshUntil: completedAt + PROJECT_CATALOG_FRESHNESS_MS,
      }));
      return { status: 'succeeded', snapshot, value: projects };
    } catch (error) {
      if (
        requestGenerations.get(key) !== generation
        || dependencies.environment.revision(environment) !== environmentRevision
      ) {
        const snapshot = getSnapshot(environment);
        return { status: 'succeeded', snapshot, value: snapshot.projects };
      }
      return failed(environment, error, 'catalog');
    }
  };

  const refresh = async (
    environment: EnvironmentRef,
    reconcile: boolean,
    reason: ProjectRefreshReason | 'ensure' | 'copy',
  ): Promise<ProjectWorkspaceResult> => {
    const current = getSnapshot(environment);
    if (
      reason === 'focus'
      && current.completeness === 'complete'
      && current.freshUntil !== null
      && now() < current.freshUntil
    ) {
      return { status: 'succeeded', snapshot: current, value: current.projects };
    }
    const expectedContext = reconcile ? dependencies.catalogObserver.captureContext() : null;
    const key = environmentKey(environment);
    const environmentRevision = dependencies.environment.revision(environment);
    let active = inFlightReads.get(key);
    if (!active || active.environmentRevision !== environmentRevision) {
      const request = readProjects(environment, environmentRevision).finally(() => {
        if (inFlightReads.get(key)?.request === request) inFlightReads.delete(key);
      });
      active = { environmentRevision, request };
      inFlightReads.set(key, active);
    }
    const result = await active.request;
    if (
      expectedContext
      && result.status === 'succeeded'
      && result.snapshot.completeness === 'complete'
      && result.snapshot.environmentRevision === environmentRevision
    ) {
      publishCompleteSnapshot(expectedContext, environment, result.snapshot.projects);
    }
    return result;
  };

  const ensureLoaded = (environment: EnvironmentRef): Promise<ProjectWorkspaceResult> => {
    const snapshot = getSnapshot(environment);
    if (
      snapshot.completeness === 'complete'
      && snapshot.freshUntil !== null
      && now() < snapshot.freshUntil
    ) {
      return Promise.resolve({ status: 'succeeded', snapshot, value: snapshot.projects });
    }
    return refresh(environment, true, 'ensure');
  };

  const executeWrite = async <T>(
    environment: EnvironmentRef,
    operation: () => Promise<T>,
    apply: (
      current: ProjectWorkspaceSnapshot,
      value: T,
    ) => Pick<ProjectWorkspaceSnapshot, 'projects' | 'completeness'>,
    reconcile: boolean,
    expectedContext = dependencies.catalogObserver.captureContext(),
  ): Promise<ProjectWorkspaceResult> => {
    try {
      const outcome = await dependencies.write.run(operation);
      if (outcome.status === 'notRun') return { status: 'notRun', reason: 'writeBlocked' };
      invalidateRequests(environment);
      const completedAt = now();
      const snapshot = commit(environment, (current) => ({
        ...current,
        ...apply(current, outcome.value),
        phase: 'ready',
        error: null,
        environmentRevision: dependencies.environment.revision(environment),
        lastSuccessAt: completedAt,
        freshUntil: completedAt + PROJECT_CATALOG_FRESHNESS_MS,
      }));
      if (reconcile && snapshot.completeness === 'complete') {
        publishCompleteSnapshot(expectedContext, environment, snapshot.projects);
      }
      return {
        status: 'succeeded',
        snapshot,
        value: outcome.value as ProjectWorkspaceValue,
      };
    } catch (error) {
      return commandFailed(environment, error);
    }
  };

  return {
    getSnapshot,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    async execute(command) {
      switch (command.kind) {
        case 'ensureLoaded':
          return ensureLoaded(command.environment);
        case 'refresh':
          return refresh(command.environment, true, command.reason ?? 'manual');
        case 'prepareCopyTarget':
          try {
            await dependencies.environment.ensureAvailable(command.environment);
          } catch (error) {
            return failed(command.environment, error, 'environment');
          }
          return refresh(command.environment, false, 'copy');
        case 'add':
          if (getSnapshot(command.environment).completeness !== 'complete') {
            return { status: 'notRun', reason: 'catalogNotReady' };
          }
          return executeWrite(
            command.environment,
            () => dependencies.backend.add(command.environment, command.nativePath),
            (current, result) => ({
              projects: upsertProject(current.projects, result.project),
              completeness: current.completeness,
            }),
            false,
          );
        case 'remove':
          return executeWrite(
            command.environment,
            () => dependencies.backend.remove(command.environment, command.projectId),
            (_current, projects) => ({ projects, completeness: 'complete' }),
            true,
            command.expectedContext,
          );
        case 'setCrossStorageWarning':
          return executeWrite(
            command.environment,
            () => dependencies.backend.setCrossStorageWarning(
              command.environment,
              command.projectId,
              command.suppressed,
            ),
            (current, project) => ({
              projects: upsertProject(current.projects, project),
              completeness: current.completeness,
            }),
            false,
          );
      }
    },
  };
}
