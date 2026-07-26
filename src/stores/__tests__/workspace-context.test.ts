import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { EnvironmentRef, ProjectInfo } from '@/bindings';
import { useEnvironmentStore } from '../environment';
import { useProjectStore } from '../projects';
import { useWorkspaceContextStore } from '../workspace-context';

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
      pendingEnvironment: null,
      contextRevision: 0,
    });
  });

  it('starts at Host Global and increments revision only for committed changes', () => {
    expect(useWorkspaceContextStore.getState()).toMatchObject({
      selectedContext: { environment: host, scope: { scope: 'global' } },
      pendingEnvironment: null,
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
    expect(useWorkspaceContextStore.getState().pendingEnvironment).toEqual(ubuntu);
    await expect(useWorkspaceContextStore.getState().switchEnvironment(debian))
      .rejects.toThrow('Environment switch already in progress');

    connecting.resolve();
    await vi.waitFor(() => expect(refresh).toHaveBeenCalledWith(ubuntu));
    expect(useWorkspaceContextStore.getState()).toMatchObject({
      selectedContext: { environment: ubuntu, scope: { scope: 'global' } },
      pendingEnvironment: null,
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
      pendingEnvironment: null,
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
      pendingEnvironment: null,
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
      pendingEnvironment: null,
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
      pendingEnvironment: null,
      contextRevision: 4,
    });
  });
});
