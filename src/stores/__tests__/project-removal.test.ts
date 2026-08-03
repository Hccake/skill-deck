import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { EnvironmentRef, ProjectInfo } from '@/bindings';
import { useProjectStore } from '../projects';
import { useWorkspaceContextStore } from '../workspace-context';
import { captureProjectRemoval, confirmProjectRemoval } from '../project-removal';

const ubuntu: EnvironmentRef = { kind: 'wsl', distro_name: 'Ubuntu' };
const project: ProjectInfo = {
  binding: {
    id: 'project-a',
    nativePath: '/work/project-a',
    displayName: null,
    order: null,
    suppressCrossStorageWarning: false,
  },
  storage: { access: 'native', owner: ubuntu },
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

describe('project removal coordinator', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    useWorkspaceContextStore.setState({
      selectedContext: {
        environment: ubuntu,
        scope: { scope: 'project', project_id: 'project-a' },
      },
      transition: { kind: 'idle' },
      contextRevision: 4,
    });
  });

  it('captures an immutable request with a user-facing project name', () => {
    expect(captureProjectRemoval(ubuntu, project, 4)).toEqual({
      environment: ubuntu,
      projectId: 'project-a',
      projectName: 'project-a',
      contextRevision: 4,
    });
  });

  it('returns to Global when the removed project is still the committed context', async () => {
    const remove = vi.spyOn(useProjectStore.getState(), 'remove').mockResolvedValue([]);

    await confirmProjectRemoval(captureProjectRemoval(ubuntu, project, 4));

    expect(remove).toHaveBeenCalledWith(ubuntu, 'project-a');
    expect(useWorkspaceContextStore.getState()).toMatchObject({
      selectedContext: { environment: ubuntu, scope: { scope: 'global' } },
      contextRevision: 5,
    });
  });

  it('does not overwrite a context change made while removal is running', async () => {
    const pendingRemoval = deferred<ProjectInfo[]>();
    vi.spyOn(useProjectStore.getState(), 'remove').mockReturnValue(pendingRemoval.promise);
    const removal = confirmProjectRemoval(captureProjectRemoval(ubuntu, project, 4));

    useWorkspaceContextStore.getState().selectProject('project-b');
    pendingRemoval.resolve([]);
    await removal;

    expect(useWorkspaceContextStore.getState()).toMatchObject({
      selectedContext: {
        environment: ubuntu,
        scope: { scope: 'project', project_id: 'project-b' },
      },
      contextRevision: 5,
    });
  });

  it('keeps the selected project when removal was not executed', async () => {
    vi.spyOn(useProjectStore.getState(), 'remove').mockResolvedValue(null);

    await expect(confirmProjectRemoval(captureProjectRemoval(ubuntu, project, 4)))
      .resolves.toBe(false);

    expect(useWorkspaceContextStore.getState().selectedContext.scope)
      .toEqual({ scope: 'project', project_id: 'project-a' });
  });
});
