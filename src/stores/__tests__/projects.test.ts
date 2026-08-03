import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ActiveMutation, EnvironmentRef, ProjectInfo } from '@/bindings';
import { useMutationStore } from '../mutation';
import { useProjectStore } from '../projects';
import { useInstallWizardSessionStore } from '../install-wizard-session';

const mocks = vi.hoisted(() => ({
  listEnvironmentProjects: vi.fn(),
  addEnvironmentProject: vi.fn(),
  removeEnvironmentProject: vi.fn(),
  setEnvironmentProjectCrossStorageWarning: vi.fn(),
  getInstallWizardSession: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  listEnvironmentProjects: (...args: unknown[]) => mocks.listEnvironmentProjects(...args),
  addEnvironmentProject: (...args: unknown[]) => mocks.addEnvironmentProject(...args),
  removeEnvironmentProject: (...args: unknown[]) => mocks.removeEnvironmentProject(...args),
  setEnvironmentProjectCrossStorageWarning: (...args: unknown[]) => (
    mocks.setEnvironmentProjectCrossStorageWarning(...args)
  ),
  getInstallWizardSession: () => mocks.getInstallWizardSession(),
}));

const ubuntu: EnvironmentRef = { kind: 'wsl', distro_name: 'Ubuntu' };
const project = (id: string, nativePath: string): ProjectInfo => ({
  binding: {
    id,
    nativePath,
    displayName: null,
    order: null,
    suppressCrossStorageWarning: false,
  },
  storage: { access: 'native', owner: ubuntu },
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

const activeMutation: ActiveMutation = {
  id: 'mutation-1',
  kind: 'update',
  context: { environment: { kind: 'host' }, scope: { scope: 'global' } },
  phase: 'preparing',
  progress: null,
  cancelable: true,
};

describe('useProjectStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useProjectStore.setState({
      projectsByEnvironment: {},
      loadStateByEnvironment: {},
      errorsByEnvironment: {},
    });
    useMutationStore.setState({
      revision: 0,
      activeMutation: null,
      cancelling: false,
      loading: false,
    });
    useInstallWizardSessionStore.setState({
      revision: 0, active: false, loading: false, hasConfirmedSnapshot: false,
      syncError: null, monitorRetryRevision: 0, snapshotVersion: 0,
    });
    mocks.getInstallWizardSession.mockResolvedValue({ revision: 1, active: true });
  });

  it('keeps the newest refresh result for one environment', async () => {
    const first = deferred<ProjectInfo[]>();
    const second = deferred<ProjectInfo[]>();
    mocks.listEnvironmentProjects
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    const firstRefresh = useProjectStore.getState().refresh(ubuntu);
    const secondRefresh = useProjectStore.getState().refresh(ubuntu);
    second.resolve([project('new', '/work/new')]);
    await secondRefresh;
    first.resolve([project('old', '/work/old')]);
    await firstRefresh;

    expect(useProjectStore.getState().projectsByEnvironment['wsl:ubuntu'])
      .toEqual([project('new', '/work/new')]);
  });

  it('passes the captured environment and raw picker path to backend', async () => {
    const authoritative = {
      ...project('app', '/mnt/d/Code/app'),
      storage: { access: 'crossStorage' as const, owner: { kind: 'host' as const } },
    };
    useProjectStore.setState({
      projectsByEnvironment: { 'wsl:ubuntu': [project('app', '/old')] },
    });
    mocks.addEnvironmentProject.mockResolvedValue({
      project: authoritative,
      created: false,
    });

    const result = await useProjectStore.getState().add(ubuntu, 'D:\\Code\\app');

    expect(mocks.addEnvironmentProject).toHaveBeenCalledWith(ubuntu, 'D:\\Code\\app');
    expect(result?.created).toBe(false);
    expect(useProjectStore.getState().projectsByEnvironment['wsl:ubuntu'])
      .toEqual([authoritative]);
  });

  it('does not let an older refresh overwrite a successful project write', async () => {
    const refreshResult = deferred<ProjectInfo[]>();
    const authoritative = project('app', '/work/app');
    mocks.listEnvironmentProjects.mockReturnValue(refreshResult.promise);
    mocks.addEnvironmentProject.mockResolvedValue({
      project: authoritative,
      created: true,
    });

    const refresh = useProjectStore.getState().refresh(ubuntu);
    await useProjectStore.getState().add(ubuntu, '/work/app');
    refreshResult.resolve([project('old', '/work/old')]);
    await refresh;

    expect(useProjectStore.getState().projectsByEnvironment['wsl:ubuntu'])
      .toEqual([authoritative]);
  });

  it('writes remove and warning results only to the explicit environment key', async () => {
    const suppressed = {
      ...project('app', '/work/app'),
      binding: {
        ...project('app', '/work/app').binding,
        suppressCrossStorageWarning: true,
      },
    };
    mocks.removeEnvironmentProject.mockResolvedValue([]);
    mocks.setEnvironmentProjectCrossStorageWarning.mockResolvedValue(suppressed);
    useProjectStore.setState({
      projectsByEnvironment: { 'wsl:ubuntu': [project('app', '/work/app')] },
    });

    await useProjectStore.getState().setCrossStorageWarning(ubuntu, 'app', true);
    expect(useProjectStore.getState().projectsByEnvironment['wsl:ubuntu']).toEqual([suppressed]);
    await useProjectStore.getState().remove(ubuntu, 'app');

    expect(mocks.setEnvironmentProjectCrossStorageWarning).toHaveBeenCalledWith(
      ubuntu,
      'app',
      true,
    );
    expect(mocks.removeEnvironmentProject).toHaveBeenCalledWith(ubuntu, 'app');
    expect(useProjectStore.getState().projectsByEnvironment['wsl:ubuntu']).toEqual([]);
  });

  it('blocks project writes while another mutation is active', async () => {
    useMutationStore.setState({ activeMutation });

    await expect(useProjectStore.getState().add(ubuntu, '/work/app'))
      .rejects.toThrow('Another write operation is already running');
    await expect(useProjectStore.getState().remove(ubuntu, 'app'))
      .rejects.toThrow('Another write operation is already running');
    await expect(useProjectStore.getState().setCrossStorageWarning(ubuntu, 'app', true))
      .rejects.toThrow('Another write operation is already running');
  });

  it('blocks project writes while the install wizard session is active', async () => {
    useInstallWizardSessionStore.setState({ revision: 1, active: true });

    await expect(useProjectStore.getState().add(ubuntu, '/work/app'))
      .rejects.toThrow('Another write operation is already running');
    expect(mocks.addEnvironmentProject).not.toHaveBeenCalled();
  });

  it('returns null without changing projects when installation wins add admission', async () => {
    const existing = [project('existing', '/work/existing')];
    useProjectStore.setState({ projectsByEnvironment: { 'wsl:ubuntu': existing } });
    mocks.addEnvironmentProject.mockRejectedValue({ kind: 'installWizardActive' });

    await expect(useProjectStore.getState().add(ubuntu, '/work/app')).resolves.toBeNull();

    expect(useProjectStore.getState().projectsByEnvironment['wsl:ubuntu']).toEqual(existing);
    expect(useProjectStore.getState().errorsByEnvironment['wsl:ubuntu']).toBeUndefined();
  });
});
