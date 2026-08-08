import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { EnvironmentRef } from '@/bindings';
import * as tauriApi from '@/hooks/useTauriApi';
import { useEnvironmentStore } from '../environment';
import { projectWorkspace } from '../projects';
import { selectPendingEnvironment, useWorkspaceContextStore } from '../workspace-context';

const native: EnvironmentRef = { kind: 'native' };
const ubuntu: EnvironmentRef = { kind: 'wsl', distro_name: 'Ubuntu' };
const debian: EnvironmentRef = { kind: 'wsl', distro_name: 'Debian' };

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((onResolve, onReject) => {
    resolve = onResolve;
    reject = onReject;
  });
  return { promise, resolve, reject };
}

describe('useWorkspaceContextStore', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    useWorkspaceContextStore.setState({
      selectedContext: { environment: native, scope: { scope: 'global' } },
      transition: { kind: 'idle' },
      wslIntegrationFailure: null,
      contextRevision: 0,
    });
  });

  it('starts at Native Global and increments revision only for committed changes', () => {
    expect(useWorkspaceContextStore.getState()).toMatchObject({
      selectedContext: { environment: native, scope: { scope: 'global' } },
      transition: { kind: 'idle' },
      contextRevision: 0,
    });

    useWorkspaceContextStore.getState().selectProject('project-a');
    useWorkspaceContextStore.getState().selectProject('project-a');
    expect(useWorkspaceContextStore.getState().contextRevision).toBe(1);
    useWorkspaceContextStore.getState().selectGlobal();
    useWorkspaceContextStore.getState().selectGlobal();
    expect(useWorkspaceContextStore.getState().contextRevision).toBe(2);
  });

  it('commits the environment as soon as connection finishes', async () => {
    const connecting = deferred<void>();
    const refreshing = deferred<Awaited<ReturnType<typeof projectWorkspace.execute>>>();
    const connect = vi.spyOn(useEnvironmentStore.getState(), 'connect')
      .mockReturnValue(connecting.promise);
    const execute = vi.spyOn(projectWorkspace, 'execute')
      .mockReturnValue(refreshing.promise);

    const switching = useWorkspaceContextStore.getState().switchEnvironment(ubuntu);

    expect(useWorkspaceContextStore.getState().selectedContext.environment).toEqual(native);
    expect(selectPendingEnvironment(useWorkspaceContextStore.getState())).toEqual(ubuntu);
    await expect(useWorkspaceContextStore.getState().switchEnvironment(debian))
      .rejects.toThrow('Workspace transition already in progress');

    connecting.resolve();
    await vi.waitFor(() => expect(execute).toHaveBeenCalledWith({
      kind: 'refresh',
      environment: ubuntu,
      reason: 'reconnect',
    }));
    expect(useWorkspaceContextStore.getState()).toMatchObject({
      selectedContext: { environment: ubuntu, scope: { scope: 'global' } },
      transition: { kind: 'idle' },
      contextRevision: 1,
    });
    refreshing.resolve({
      status: 'succeeded',
      snapshot: projectWorkspace.getSnapshot(ubuntu),
      value: [],
    });
    await switching;

    expect(connect).toHaveBeenCalledWith(ubuntu);
  });

  it('keeps the committed environment when project refresh fails', async () => {
    const error = new Error('project registry unavailable');
    vi.spyOn(useEnvironmentStore.getState(), 'connect').mockResolvedValue(undefined);
    vi.spyOn(projectWorkspace, 'execute').mockResolvedValue({
      status: 'failed',
      failureSource: 'catalog',
      error: { kind: 'custom', data: { message: error.message } },
      snapshot: projectWorkspace.getSnapshot(ubuntu),
    });

    await useWorkspaceContextStore.getState().switchEnvironment(ubuntu);

    expect(useWorkspaceContextStore.getState()).toMatchObject({
      selectedContext: { environment: ubuntu, scope: { scope: 'global' } },
      transition: { kind: 'idle' },
      contextRevision: 1,
    });
  });

  it('preserves the previous context and clears pending state after failure', async () => {
    const error = new Error('distribution unavailable');
    vi.spyOn(useEnvironmentStore.getState(), 'connect').mockRejectedValue(error);
    const execute = vi.spyOn(projectWorkspace, 'execute');

    await expect(useWorkspaceContextStore.getState().switchEnvironment(ubuntu))
      .rejects.toThrow('distribution unavailable');

    expect(execute).not.toHaveBeenCalled();
    expect(useWorkspaceContextStore.getState()).toMatchObject({
      selectedContext: { environment: native, scope: { scope: 'global' } },
      transition: { kind: 'idle' },
      contextRevision: 0,
    });
  });

  it('reconnects the current environment without leaving the selected project', async () => {
    useWorkspaceContextStore.setState({
      selectedContext: {
        environment: native,
        scope: { scope: 'project', project_id: 'project-a' },
      },
      contextRevision: 4,
    });
    const connect = vi.spyOn(useEnvironmentStore.getState(), 'connect')
      .mockResolvedValue(undefined);
    const execute = vi.spyOn(projectWorkspace, 'execute').mockResolvedValue({
      status: 'succeeded',
      snapshot: projectWorkspace.getSnapshot(native),
      value: [],
    });

    await useWorkspaceContextStore.getState().switchEnvironment(native);

    expect(connect).toHaveBeenCalledWith(native);
    expect(execute).toHaveBeenCalledWith({
      kind: 'refresh',
      environment: native,
      reason: 'reconnect',
    });
    expect(useWorkspaceContextStore.getState()).toMatchObject({
      selectedContext: {
        environment: native,
        scope: { scope: 'project', project_id: 'project-a' },
      },
      transition: { kind: 'idle' },
      contextRevision: 4,
    });
  });

  it('keeps the selected project when reconnecting the current environment fails', async () => {
    const error = new Error('native runtime unavailable');
    useWorkspaceContextStore.setState({
      selectedContext: {
        environment: native,
        scope: { scope: 'project', project_id: 'project-a' },
      },
      contextRevision: 4,
    });
    vi.spyOn(useEnvironmentStore.getState(), 'connect').mockRejectedValue(error);
    const execute = vi.spyOn(projectWorkspace, 'execute');

    await expect(useWorkspaceContextStore.getState().switchEnvironment(native))
      .rejects.toThrow('native runtime unavailable');

    expect(execute).not.toHaveBeenCalled();
    expect(useWorkspaceContextStore.getState()).toMatchObject({
      selectedContext: {
        environment: native,
        scope: { scope: 'project', project_id: 'project-a' },
      },
      transition: { kind: 'idle' },
      contextRevision: 4,
    });
  });

  it('ignores a complete catalog captured before the user changes Context', async () => {
    const listing = deferred<Awaited<ReturnType<typeof tauriApi.listEnvironmentProjects>>>();
    vi.spyOn(tauriApi, 'listEnvironmentProjects').mockReturnValue(listing.promise);
    useWorkspaceContextStore.setState({
      selectedContext: {
        environment: native,
        scope: { scope: 'project', project_id: 'removed' },
      },
      contextRevision: 7,
    });

    const refresh = projectWorkspace.execute({
      kind: 'refresh',
      environment: native,
      reason: 'manual',
    });
    useWorkspaceContextStore.getState().selectProject('new-selection');
    listing.resolve([]);
    await refresh;

    expect(useWorkspaceContextStore.getState()).toMatchObject({
      selectedContext: {
        environment: native,
        scope: { scope: 'project', project_id: 'new-selection' },
      },
      contextRevision: 8,
    });
  });

  it('owns Native switching and setting persistence as one WSL transition', async () => {
    useWorkspaceContextStore.setState({
      selectedContext: { environment: ubuntu, scope: { scope: 'global' } },
    });
    const connecting = deferred<void>();
    const persisting = deferred<boolean>();
    vi.spyOn(useEnvironmentStore.getState(), 'connect').mockReturnValue(connecting.promise);
    const setEnabled = vi.spyOn(useEnvironmentStore.getState(), 'setWslIntegrationEnabled')
      .mockReturnValue(persisting.promise);
    vi.spyOn(projectWorkspace, 'execute').mockResolvedValue({
      status: 'succeeded',
      snapshot: projectWorkspace.getSnapshot(native),
      value: [],
    });

    const disabling = useWorkspaceContextStore.getState().changeWslIntegration(false);
    expect(useWorkspaceContextStore.getState().transition).toEqual({
      kind: 'wslIntegration',
      phase: 'switchingNative',
    });
    await expect(useWorkspaceContextStore.getState().switchEnvironment(debian))
      .rejects.toThrow('Workspace transition already in progress');

    connecting.resolve();
    await vi.waitFor(() => expect(setEnabled).toHaveBeenCalledWith(false));
    expect(useWorkspaceContextStore.getState()).toMatchObject({
      selectedContext: { environment: native, scope: { scope: 'global' } },
      transition: { kind: 'wslIntegration', phase: 'disabling' },
    });

    persisting.resolve(true);
    await expect(disabling).resolves.toEqual({ status: 'succeeded' });
    expect(useWorkspaceContextStore.getState().transition).toEqual({ kind: 'idle' });
  });

  it('returns the failing stage and skips persistence when Native switching fails', async () => {
    useWorkspaceContextStore.setState({
      selectedContext: { environment: ubuntu, scope: { scope: 'global' } },
    });
    vi.spyOn(useEnvironmentStore.getState(), 'connect')
      .mockRejectedValue(new Error('native unavailable'));
    const setEnabled = vi.spyOn(useEnvironmentStore.getState(), 'setWslIntegrationEnabled');

    const outcome = await useWorkspaceContextStore.getState().changeWslIntegration(false);

    expect(outcome).toEqual({
      status: 'failed',
      failure: {
        stage: 'switchHost',
        error: { kind: 'custom', data: { message: 'native unavailable' } },
      },
    });
    expect(setEnabled).not.toHaveBeenCalled();
    expect(useWorkspaceContextStore.getState().transition).toEqual({ kind: 'idle' });
  });

  it('returns a structured backend busy result', async () => {
    const error = {
      kind: 'wslIntegrationBusy' as const,
      data: { reason: 'wslOperation' as const },
    };
    vi.spyOn(useEnvironmentStore.getState(), 'setWslIntegrationEnabled').mockRejectedValue(error);

    await expect(
      useWorkspaceContextStore.getState().changeWslIntegration(true),
    ).resolves.toEqual({
      status: 'failed',
      failure: { stage: 'busy', error },
    });
    expect(useWorkspaceContextStore.getState().transition).toEqual({ kind: 'idle' });
  });

  it('reports that the WSL setting was not changed after installation wins admission', async () => {
    vi.spyOn(useEnvironmentStore.getState(), 'setWslIntegrationEnabled')
      .mockResolvedValue(false);

    await expect(useWorkspaceContextStore.getState().changeWslIntegration(true))
      .resolves.toEqual({ status: 'notRun' });

    expect(useWorkspaceContextStore.getState()).toMatchObject({
      transition: { kind: 'idle' },
      wslIntegrationFailure: null,
    });
  });

  it('returns a structured busy result when another workspace transition owns admission', async () => {
    const connecting = deferred<void>();
    vi.spyOn(useEnvironmentStore.getState(), 'connect').mockReturnValue(connecting.promise);

    const switching = useWorkspaceContextStore.getState().switchEnvironment(ubuntu);
    await expect(
      useWorkspaceContextStore.getState().changeWslIntegration(false),
    ).resolves.toEqual({
      status: 'failed',
      failure: { stage: 'busy', error: { kind: 'mutationBusy' } },
    });

    connecting.resolve();
    await switching;
  });
});
