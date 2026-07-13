import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useEnvironmentStore } from '../environment';
import type { EnvironmentInfo, ProjectBinding } from '@/bindings';

const mocks = vi.hoisted(() => ({
  listEnvironments: vi.fn(),
  connectEnvironment: vi.fn(),
  listEnvironmentProjects: vi.fn(),
  addEnvironmentProject: vi.fn(),
  removeEnvironmentProject: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  listEnvironments: (...args: unknown[]) => mocks.listEnvironments(...args),
  connectEnvironment: (...args: unknown[]) => mocks.connectEnvironment(...args),
  listEnvironmentProjects: (...args: unknown[]) => mocks.listEnvironmentProjects(...args),
  addEnvironmentProject: (...args: unknown[]) => mocks.addEnvironmentProject(...args),
  removeEnvironmentProject: (...args: unknown[]) => mocks.removeEnvironmentProject(...args),
}));

const host: EnvironmentInfo = {
  environment: { kind: 'host' },
  displayName: 'Windows',
  status: 'available',
};
const ubuntu: EnvironmentInfo = {
  environment: { kind: 'wsl', distro_name: 'Ubuntu' },
  displayName: 'Ubuntu',
  status: 'available',
};
const ubuntuProjects: ProjectBinding[] = [{
  id: 'ubuntu-project',
  nativePath: '/work/app',
  displayName: null,
  order: null,
  suppressCrossStorageWarning: false,
}];

describe('useEnvironmentStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useEnvironmentStore.setState({
      environments: [],
      selectedEnvironment: { kind: 'host' },
      projectsByEnvironment: {},
      projectsLoaded: {},
      discoveryState: 'idle',
      errors: {},
    });
  });

  it('discovers WSL entries without connecting any distro', async () => {
    mocks.listEnvironments.mockResolvedValue([host, ubuntu]);

    await useEnvironmentStore.getState().discoverEnvironments();

    expect(mocks.listEnvironments).toHaveBeenCalledTimes(1);
    expect(mocks.connectEnvironment).not.toHaveBeenCalled();
    expect(useEnvironmentStore.getState().environments).toEqual([host, ubuntu]);
  });

  it('connects and loads projects only after selecting a WSL environment', async () => {
    mocks.listEnvironments.mockResolvedValue([host, ubuntu]);
    mocks.connectEnvironment.mockResolvedValue({ distroName: 'Ubuntu' });
    mocks.listEnvironmentProjects.mockResolvedValue(ubuntuProjects);
    await useEnvironmentStore.getState().discoverEnvironments();

    await useEnvironmentStore.getState().selectEnvironment(ubuntu.environment);

    expect(mocks.connectEnvironment).toHaveBeenCalledWith('Ubuntu');
    expect(mocks.listEnvironmentProjects).toHaveBeenCalledWith(ubuntu.environment);
    expect(useEnvironmentStore.getState().selectedEnvironment).toEqual(ubuntu.environment);
  });

  it('shows a connecting state while a WSL session is being opened', async () => {
    let finishConnect: (() => void) | undefined;
    mocks.listEnvironments.mockResolvedValue([host, ubuntu]);
    mocks.connectEnvironment.mockImplementation(() => new Promise<void>((resolve) => {
      finishConnect = resolve;
    }));
    mocks.listEnvironmentProjects.mockResolvedValue(ubuntuProjects);
    await useEnvironmentStore.getState().discoverEnvironments();

    const selection = useEnvironmentStore.getState().selectEnvironment(ubuntu.environment);

    expect(useEnvironmentStore.getState().environments[1].status).toBe('connecting');
    finishConnect?.();
    await selection;
    expect(useEnvironmentStore.getState().environments[1].status).toBe('available');
  });

  it('keeps discovered entries and marks the selected environment unavailable on query failure', async () => {
    mocks.listEnvironments.mockResolvedValue([host, ubuntu]);
    await useEnvironmentStore.getState().discoverEnvironments();
    mocks.connectEnvironment.mockRejectedValue(new Error('WSL is unavailable'));

    await expect(
      useEnvironmentStore.getState().selectEnvironment(ubuntu.environment)
    ).rejects.toThrow('WSL is unavailable');

    const state = useEnvironmentStore.getState();
    expect(state.environments).toHaveLength(2);
    expect(state.environments[1].status).toBe('unavailable');
    expect(state.environments[0]).toEqual(host);
  });

  it('uses an explicit environment snapshot for project writes', async () => {
    mocks.addEnvironmentProject.mockResolvedValue(ubuntuProjects);

    await useEnvironmentStore.getState().addProject('/work/app', ubuntu.environment);

    expect(mocks.addEnvironmentProject).toHaveBeenCalledWith(ubuntu.environment, '/work/app');
    expect(useEnvironmentStore.getState().projectsByEnvironment['wsl:Ubuntu']).toEqual(ubuntuProjects);
  });
});
