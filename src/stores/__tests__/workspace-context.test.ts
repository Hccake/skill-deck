import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { EnvironmentRef, ProjectInfo } from '@/bindings';
import { useEnvironmentStore } from '../environment';
import { useProjectStore } from '../projects';
import { selectPendingEnvironment, useWorkspaceContextStore } from '../workspace-context';

const host: EnvironmentRef = { kind: 'host' };
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
      selectedContext: { environment: host, scope: { scope: 'global' } },
      transition: { kind: 'idle' },
      wslIntegrationFailure: null,
      contextRevision: 0,
    });
  });

  it('starts at Host Global and increments revision only for committed changes', () => {
    expect(useWorkspaceContextStore.getState()).toMatchObject({
      selectedContext: { environment: host, scope: { scope: 'global' } },
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
    const refreshing = deferred<ProjectInfo[]>();
    const connect = vi.spyOn(useEnvironmentStore.getState(), 'connect')
      .mockReturnValue(connecting.promise);
    const refresh = vi.spyOn(useProjectStore.getState(), 'refresh')
      .mockReturnValue(refreshing.promise);

    const switching = useWorkspaceContextStore.getState().switchEnvironment(ubuntu);

    expect(useWorkspaceContextStore.getState().selectedContext.environment).toEqual(host);
    expect(selectPendingEnvironment(useWorkspaceContextStore.getState())).toEqual(ubuntu);
    await expect(useWorkspaceContextStore.getState().switchEnvironment(debian))
      .rejects.toThrow('Workspace transition already in progress');

    connecting.resolve();
    await vi.waitFor(() => expect(refresh).toHaveBeenCalledWith(ubuntu));
    expect(useWorkspaceContextStore.getState()).toMatchObject({
      selectedContext: { environment: ubuntu, scope: { scope: 'global' } },
      transition: { kind: 'idle' },
      contextRevision: 1,
    });
    refreshing.resolve([]);
    await switching;

    expect(connect).toHaveBeenCalledWith(ubuntu);
  });

  it('keeps the committed environment when project refresh fails', async () => {
    const error = new Error('project registry unavailable');
    vi.spyOn(useEnvironmentStore.getState(), 'connect').mockResolvedValue(undefined);
    vi.spyOn(useProjectStore.getState(), 'refresh').mockRejectedValue(error);

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
    const refresh = vi.spyOn(useProjectStore.getState(), 'refresh');

    await expect(useWorkspaceContextStore.getState().switchEnvironment(ubuntu))
      .rejects.toThrow('distribution unavailable');

    expect(refresh).not.toHaveBeenCalled();
    expect(useWorkspaceContextStore.getState()).toMatchObject({
      selectedContext: { environment: host, scope: { scope: 'global' } },
      transition: { kind: 'idle' },
      contextRevision: 0,
    });
  });

  it('reconnects the current environment without leaving the selected project', async () => {
    useWorkspaceContextStore.setState({
      selectedContext: {
        environment: host,
        scope: { scope: 'project', project_id: 'project-a' },
      },
      contextRevision: 4,
    });
    const connect = vi.spyOn(useEnvironmentStore.getState(), 'connect')
      .mockResolvedValue(undefined);
    const refresh = vi.spyOn(useProjectStore.getState(), 'refresh')
      .mockResolvedValue([]);

    await useWorkspaceContextStore.getState().switchEnvironment(host);

    expect(connect).toHaveBeenCalledWith(host);
    expect(refresh).toHaveBeenCalledWith(host);
    expect(useWorkspaceContextStore.getState()).toMatchObject({
      selectedContext: {
        environment: host,
        scope: { scope: 'project', project_id: 'project-a' },
      },
      transition: { kind: 'idle' },
      contextRevision: 4,
    });
  });

  it('keeps the selected project when reconnecting the current environment fails', async () => {
    const error = new Error('host runtime unavailable');
    useWorkspaceContextStore.setState({
      selectedContext: {
        environment: host,
        scope: { scope: 'project', project_id: 'project-a' },
      },
      contextRevision: 4,
    });
    vi.spyOn(useEnvironmentStore.getState(), 'connect').mockRejectedValue(error);
    const refresh = vi.spyOn(useProjectStore.getState(), 'refresh');

    await expect(useWorkspaceContextStore.getState().switchEnvironment(host))
      .rejects.toThrow('host runtime unavailable');

    expect(refresh).not.toHaveBeenCalled();
    expect(useWorkspaceContextStore.getState()).toMatchObject({
      selectedContext: {
        environment: host,
        scope: { scope: 'project', project_id: 'project-a' },
      },
      transition: { kind: 'idle' },
      contextRevision: 4,
    });
  });

  it('owns Host switching and setting persistence as one WSL transition', async () => {
    useWorkspaceContextStore.setState({
      selectedContext: { environment: ubuntu, scope: { scope: 'global' } },
    });
    const connecting = deferred<void>();
    const persisting = deferred<boolean>();
    vi.spyOn(useEnvironmentStore.getState(), 'connect').mockReturnValue(connecting.promise);
    const setEnabled = vi.spyOn(useEnvironmentStore.getState(), 'setWslIntegrationEnabled')
      .mockReturnValue(persisting.promise);
    vi.spyOn(useProjectStore.getState(), 'refresh').mockResolvedValue([]);

    const disabling = useWorkspaceContextStore.getState().changeWslIntegration(false);
    expect(useWorkspaceContextStore.getState().transition).toEqual({
      kind: 'wslIntegration',
      phase: 'switchingHost',
    });
    await expect(useWorkspaceContextStore.getState().switchEnvironment(debian))
      .rejects.toThrow('Workspace transition already in progress');

    connecting.resolve();
    await vi.waitFor(() => expect(setEnabled).toHaveBeenCalledWith(false));
    expect(useWorkspaceContextStore.getState()).toMatchObject({
      selectedContext: { environment: host, scope: { scope: 'global' } },
      transition: { kind: 'wslIntegration', phase: 'disabling' },
    });

    persisting.resolve(true);
    await expect(disabling).resolves.toEqual({ status: 'succeeded' });
    expect(useWorkspaceContextStore.getState().transition).toEqual({ kind: 'idle' });
  });

  it('returns the failing stage and skips persistence when Host switching fails', async () => {
    useWorkspaceContextStore.setState({
      selectedContext: { environment: ubuntu, scope: { scope: 'global' } },
    });
    vi.spyOn(useEnvironmentStore.getState(), 'connect')
      .mockRejectedValue(new Error('host unavailable'));
    const setEnabled = vi.spyOn(useEnvironmentStore.getState(), 'setWslIntegrationEnabled');

    const outcome = await useWorkspaceContextStore.getState().changeWslIntegration(false);

    expect(outcome).toEqual({
      status: 'failed',
      failure: {
        stage: 'switchHost',
        error: { kind: 'custom', data: { message: 'host unavailable' } },
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
