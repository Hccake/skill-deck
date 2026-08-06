import { describe, expect, it, vi } from 'vitest';
import type {
  AddProjectResult,
  ContextRef,
  EnvironmentRef,
  ProjectInfo,
} from '@/bindings';
import {
  createProjectWorkspace,
  type ProjectWorkspaceDependencies,
} from '../workspace';

const host: EnvironmentRef = { kind: 'host' };
const ubuntu: EnvironmentRef = { kind: 'wsl', distro_name: 'Ubuntu' };

function project(id: string, environment: EnvironmentRef = host): ProjectInfo {
  return {
    binding: {
      id,
      nativePath: environment.kind === 'host' ? `C:\\Code\\${id}` : `/work/${id}`,
      displayName: null,
      order: null,
      suppressCrossStorageWarning: false,
    },
    storage: {
      access: 'native',
      owner: environment.kind === 'host' ? null : environment,
    },
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((next, fail) => {
    resolve = next;
    reject = fail;
  });
  return { promise, resolve, reject };
}

function setup(overrides: Partial<ProjectWorkspaceDependencies> = {}) {
  let context: ContextRef = { environment: host, scope: { scope: 'global' } };
  let contextRevision = 0;
  const backend = {
    list: vi.fn<(environment: EnvironmentRef) => Promise<ProjectInfo[]>>()
      .mockResolvedValue([]),
    add: vi.fn<(environment: EnvironmentRef, nativePath: string) => Promise<AddProjectResult>>(),
    remove: vi.fn<(environment: EnvironmentRef, projectId: string) => Promise<ProjectInfo[]>>()
      .mockResolvedValue([]),
    setCrossStorageWarning: vi.fn(),
  };
  const environment = {
    isAvailable: vi.fn(() => true),
    revision: vi.fn(() => 0),
    ensureAvailable: vi.fn(async () => undefined),
  };
  const contextAccess = {
    captureContext: vi.fn(() => ({ context, revision: contextRevision })),
    onCompleteSnapshot: vi.fn(({ expectedContext, projects }: {
      expectedContext: { context: ContextRef; revision: number };
      projects: readonly ProjectInfo[];
    }) => {
      if (expectedContext.revision !== contextRevision || expectedContext.context !== context) return false;
      if (context.scope.scope !== 'project') return false;
      const projectId = context.scope.project_id;
      if (projects.some((entry) => entry.binding.id === projectId)) return false;
      context = { environment: context.environment, scope: { scope: 'global' } };
      contextRevision += 1;
      return true;
    }),
  };
  const write: ProjectWorkspaceDependencies['write'] = {
    run: async <T>(operation: () => Promise<T>) => ({
      status: 'succeeded' as const,
      value: await operation(),
    }),
  };
  const dependencies: ProjectWorkspaceDependencies = {
    backend,
    environment,
    catalogObserver: contextAccess,
    write,
    ...overrides,
  };
  const workspace = createProjectWorkspace(dependencies);
  return {
    workspace,
    backend,
    environment,
    contextAccess,
    write,
    selectProject(projectId: string, targetEnvironment = host) {
      context = {
        environment: targetEnvironment,
        scope: { scope: 'project', project_id: projectId },
      };
      contextRevision += 1;
    },
    context: () => context,
  };
}

describe('Project workspace', () => {
  it('coalesces concurrent first loads for one Environment', async () => {
    const pending = deferred<ProjectInfo[]>();
    const { workspace, backend } = setup();
    backend.list.mockReturnValue(pending.promise);

    const first = workspace.execute({ kind: 'ensureLoaded', environment: host });
    const second = workspace.execute({ kind: 'ensureLoaded', environment: host });
    pending.resolve([project('app')]);

    await expect(first).resolves.toMatchObject({ status: 'succeeded' });
    await expect(second).resolves.toMatchObject({ status: 'succeeded' });
    expect(backend.list).toHaveBeenCalledOnce();
    expect(workspace.getSnapshot(host)).toMatchObject({
      phase: 'ready',
      completeness: 'complete',
      projects: [project('app')],
    });
  });

  it('coalesces an explicit refresh with an in-flight first load', async () => {
    const pending = deferred<ProjectInfo[]>();
    const { workspace, backend } = setup();
    backend.list.mockReturnValue(pending.promise);

    const firstLoad = workspace.execute({ kind: 'ensureLoaded', environment: host });
    const explicitRefresh = workspace.execute({ kind: 'refresh', environment: host });
    pending.resolve([project('app')]);

    await expect(firstLoad).resolves.toMatchObject({ status: 'succeeded' });
    await expect(explicitRefresh).resolves.toMatchObject({ status: 'succeeded' });
    expect(backend.list).toHaveBeenCalledOnce();
    expect(workspace.getSnapshot(host)).toMatchObject({
      phase: 'ready',
      completeness: 'complete',
      projects: [project('app')],
    });
  });

  it('does not add a project before the Environment has a complete snapshot', async () => {
    const { workspace, backend } = setup();

    const result = await workspace.execute({
      kind: 'add',
      environment: host,
      nativePath: 'C:\\Code\\app',
    });

    expect(result).toEqual({ status: 'notRun', reason: 'catalogNotReady' });
    expect(backend.add).not.toHaveBeenCalled();
    expect(workspace.getSnapshot(host)).toMatchObject({
      phase: 'idle',
      completeness: 'partial',
      projects: [],
    });
  });

  it('commits one complete snapshot per Environment', async () => {
    const { workspace, backend } = setup();
    backend.list.mockImplementation(async (environment) => (
      environment.kind === 'host' ? [project('host-app')] : [project('wsl-app', ubuntu)]
    ));

    await workspace.execute({ kind: 'refresh', environment: host });
    await workspace.execute({ kind: 'refresh', environment: ubuntu });

    expect(workspace.getSnapshot(host)).toMatchObject({
      phase: 'ready',
      completeness: 'complete',
      projects: [project('host-app')],
    });
    expect(workspace.getSnapshot(ubuntu)).toMatchObject({
      phase: 'ready',
      completeness: 'complete',
      projects: [project('wsl-app', ubuntu)],
    });
  });

  it('coalesces concurrent explicit refreshes', async () => {
    const pending = deferred<ProjectInfo[]>();
    const { workspace, backend } = setup();
    backend.list.mockReturnValue(pending.promise);

    const first = workspace.execute({ kind: 'refresh', environment: host });
    const second = workspace.execute({ kind: 'refresh', environment: host });
    pending.resolve([project('app')]);
    await Promise.all([first, second]);

    expect(backend.list).toHaveBeenCalledOnce();
    expect(workspace.getSnapshot(host).projects).toEqual([project('app')]);
  });

  it('refreshes a complete snapshot on focus only after its freshness expires', async () => {
    let now = 1_000;
    const { workspace, backend } = setup({ now: () => now });
    backend.list.mockResolvedValue([project('app')]);

    await workspace.execute({ kind: 'refresh', environment: host, reason: 'manual' });
    now += 299_999;
    await workspace.execute({ kind: 'refresh', environment: host, reason: 'focus' });
    expect(backend.list).toHaveBeenCalledOnce();

    now += 2;
    await workspace.execute({ kind: 'refresh', environment: host, reason: 'focus' });
    expect(backend.list).toHaveBeenCalledTimes(2);
    expect(workspace.getSnapshot(host)).toMatchObject({
      lastSuccessAt: 301_001,
      freshUntil: 601_001,
    });
  });

  it('starts a new read after the Environment revision changes and discards the old result', async () => {
    let revision = 1;
    const older = deferred<ProjectInfo[]>();
    const current = deferred<ProjectInfo[]>();
    const environment: ProjectWorkspaceDependencies['environment'] = {
      isAvailable: () => true,
      ensureAvailable: async () => undefined,
      revision: () => revision,
    };
    const { workspace, backend, contextAccess } = setup({ environment });
    backend.list
      .mockReturnValueOnce(older.promise)
      .mockReturnValueOnce(current.promise);

    const oldRefresh = workspace.execute({ kind: 'refresh', environment: host, reason: 'manual' });
    revision = 2;
    const newRefresh = workspace.execute({ kind: 'refresh', environment: host, reason: 'reconnect' });
    current.resolve([project('current')]);
    await newRefresh;
    older.resolve([project('old')]);
    await oldRefresh;

    expect(backend.list).toHaveBeenCalledTimes(2);
    expect(workspace.getSnapshot(host)).toMatchObject({
      environmentRevision: 2,
      projects: [project('current')],
    });
    expect(contextAccess.onCompleteSnapshot).toHaveBeenCalledOnce();
  });

  it('does not let a pending refresh overwrite a successful write', async () => {
    const pending = deferred<ProjectInfo[]>();
    const existing = project('existing');
    const added = project('added');
    const { workspace, backend } = setup();
    backend.list.mockResolvedValueOnce([existing]);
    await workspace.execute({ kind: 'ensureLoaded', environment: host });
    backend.list.mockReturnValueOnce(pending.promise);
    backend.add.mockResolvedValue({ project: added, created: true });

    const refresh = workspace.execute({ kind: 'refresh', environment: host });
    await workspace.execute({ kind: 'add', environment: host, nativePath: added.binding.nativePath });
    pending.resolve([project('old')]);
    await refresh;

    expect(workspace.getSnapshot(host)).toMatchObject({
      projects: [existing, added],
      completeness: 'complete',
    });
  });

  it('retains the last successful projects when refresh fails', async () => {
    const { workspace, backend } = setup();
    backend.list.mockResolvedValueOnce([project('app')]);
    await workspace.execute({ kind: 'refresh', environment: host });
    backend.list.mockRejectedValueOnce(new Error('refresh failed'));

    const result = await workspace.execute({ kind: 'refresh', environment: host });

    expect(result).toMatchObject({ status: 'failed' });
    expect(workspace.getSnapshot(host)).toMatchObject({
      phase: 'error',
      completeness: 'complete',
      projects: [project('app')],
      error: { kind: 'custom', data: { message: 'refresh failed' } },
    });
  });

  it('keeps the complete catalog ready when adding a project fails', async () => {
    const existing = project('existing');
    const { workspace, backend } = setup();
    backend.list.mockResolvedValueOnce([existing]);
    await workspace.execute({ kind: 'ensureLoaded', environment: host });
    backend.add.mockRejectedValueOnce(new Error('add failed'));

    const result = await workspace.execute({
      kind: 'add',
      environment: host,
      nativePath: 'C:\\Code\\new',
    });

    expect(result).toMatchObject({
      status: 'failed',
      error: { kind: 'custom', data: { message: 'add failed' } },
    });
    expect(workspace.getSnapshot(host)).toMatchObject({
      phase: 'ready',
      completeness: 'complete',
      projects: [existing],
      error: null,
    });
  });

  it('returns to Global only when a complete snapshot removes the still-selected Project', async () => {
    const { workspace, backend, selectProject, context, contextAccess } = setup();
    selectProject('removed');
    backend.remove.mockResolvedValue([]);

    await workspace.execute({ kind: 'remove', environment: host, projectId: 'removed' });

    expect(context().scope).toEqual({ scope: 'global' });
    expect(contextAccess.onCompleteSnapshot).toHaveBeenCalledWith(
      expect.objectContaining({
        expectedContext: expect.objectContaining({
          context: expect.objectContaining({ scope: { scope: 'project', project_id: 'removed' } }),
        }),
        environment: host,
        projects: [],
      }),
    );
  });

  it('does not overwrite a Context change made while removal is running', async () => {
    const pending = deferred<ProjectInfo[]>();
    const { workspace, backend, selectProject, context } = setup();
    selectProject('removed');
    backend.remove.mockReturnValue(pending.promise);

    const removal = workspace.execute({ kind: 'remove', environment: host, projectId: 'removed' });
    selectProject('new-selection');
    pending.resolve([]);
    await removal;

    expect(context().scope).toEqual({ scope: 'project', project_id: 'new-selection' });
  });

  it('connects an explicit Copy target without changing the current Context', async () => {
    const { workspace, backend, environment, context } = setup();
    backend.list.mockResolvedValue([project('target', ubuntu)]);

    const result = await workspace.execute({ kind: 'prepareCopyTarget', environment: ubuntu });

    expect(environment.ensureAvailable).toHaveBeenCalledWith(ubuntu);
    expect(result).toMatchObject({ status: 'succeeded' });
    expect(context()).toEqual({ environment: host, scope: { scope: 'global' } });
  });

  it('distinguishes Copy target connection failure from project catalog failure', async () => {
    const connectionError = {
      kind: 'environmentUnavailable' as const,
      data: { environment: ubuntu, message: 'stopped' },
    };
    const environment: ProjectWorkspaceDependencies['environment'] = {
      isAvailable: () => true,
      revision: () => 1,
      ensureAvailable: vi.fn().mockRejectedValue(connectionError),
    };
    const connection = setup({ environment });

    await expect(connection.workspace.execute({
      kind: 'prepareCopyTarget',
      environment: ubuntu,
    })).resolves.toMatchObject({
      status: 'failed',
      failureSource: 'environment',
      error: connectionError,
    });

    const catalog = setup();
    catalog.backend.list.mockRejectedValue(new Error('catalog failed'));
    await expect(catalog.workspace.execute({
      kind: 'prepareCopyTarget',
      environment: host,
    })).resolves.toMatchObject({
      status: 'failed',
      failureSource: 'catalog',
      error: { kind: 'custom', data: { message: 'catalog failed' } },
    });
  });

  it('does not change the snapshot when write admission returns notRun', async () => {
    const write: ProjectWorkspaceDependencies['write'] = {
      run: vi.fn(async () => ({ status: 'notRun' as const })),
    };
    const { workspace, backend } = setup({ write });
    backend.list.mockResolvedValueOnce([project('existing')]);
    await workspace.execute({ kind: 'ensureLoaded', environment: host });

    const result = await workspace.execute({
      kind: 'add',
      environment: host,
      nativePath: 'C:\\Code\\app',
    });

    expect(result).toEqual({ status: 'notRun', reason: 'writeBlocked' });
    expect(backend.add).not.toHaveBeenCalled();
    expect(workspace.getSnapshot(host)).toMatchObject({
      phase: 'ready',
      completeness: 'complete',
      projects: [project('existing')],
    });
  });
});
