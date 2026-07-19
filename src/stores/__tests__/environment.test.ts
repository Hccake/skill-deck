import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppError, EnvironmentInfo, EnvironmentRuntimeEvent } from '@/bindings';
import { useEnvironmentStore } from '../environment';

const mocks = vi.hoisted(() => ({
  listEnvironments: vi.fn(),
  connectEnvironment: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  listEnvironments: (...args: unknown[]) => mocks.listEnvironments(...args),
  connectEnvironment: (...args: unknown[]) => mocks.connectEnvironment(...args),
}));

const host: EnvironmentInfo = {
  environment: { kind: 'host' },
  displayName: 'Windows',
  status: 'available',
  revision: 1,
  error: null,
};
const ubuntu: EnvironmentInfo = {
  environment: { kind: 'wsl', distro_name: 'Ubuntu' },
  displayName: 'Ubuntu',
  status: 'available',
  revision: 1,
  error: null,
};
const debian: EnvironmentInfo = {
  environment: { kind: 'wsl', distro_name: 'Debian' },
  displayName: 'Debian',
  status: 'available',
  revision: 1,
  error: null,
};

describe('useEnvironmentStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useEnvironmentStore.setState({
      environments: [],
      runtimeByEnvironment: {},
      discoveryState: 'idle',
      discoveryError: null,
      errorsByEnvironment: {},
    });
  });

  it('discovers environments without connecting a distribution', async () => {
    mocks.listEnvironments.mockResolvedValue({ environments: [host, ubuntu], error: null });

    await useEnvironmentStore.getState().discover();

    expect(useEnvironmentStore.getState().environments).toEqual([host, ubuntu]);
    expect(useEnvironmentStore.getState().discoveryState).toBe('ready');
    expect(mocks.connectEnvironment).not.toHaveBeenCalled();
  });

  it('keeps Host usable and exposes a typed discovery error', async () => {
    const error: AppError = {
      kind: 'environmentDiscoveryFailed',
      data: { message: 'wsl.exe is blocked' },
    };
    mocks.listEnvironments.mockResolvedValue({ environments: [host], error });

    await useEnvironmentStore.getState().discover();

    expect(useEnvironmentStore.getState()).toMatchObject({
      environments: [host],
      discoveryState: 'error',
      discoveryError: error,
    });
  });

  it('treats Host connection as immediately available', async () => {
    useEnvironmentStore.setState({ environments: [host, ubuntu] });

    await useEnvironmentStore.getState().connect(host.environment);

    expect(mocks.connectEnvironment).not.toHaveBeenCalled();
    expect(useEnvironmentStore.getState().environments[0].status).toBe('available');
  });

  it('exposes WSL connecting state without owning selected context', async () => {
    let finishConnect: (() => void) | undefined;
    useEnvironmentStore.setState({ environments: [host, ubuntu] });
    mocks.connectEnvironment.mockImplementation(() => new Promise<void>((resolve) => {
      finishConnect = resolve;
    }));

    const connection = useEnvironmentStore.getState().connect(ubuntu.environment);

    expect(useEnvironmentStore.getState().environments[1].status).toBe('connecting');
    expect('selectedEnvironment' in useEnvironmentStore.getState()).toBe(false);
    finishConnect?.();
    await connection;
    expect(useEnvironmentStore.getState().environments[1].status).toBe('available');
  });

  it('stores a typed error only for the failed environment', async () => {
    const error: AppError = {
      kind: 'environmentUnavailable',
      data: { environment: ubuntu.environment, message: 'distribution stopped' },
    };
    useEnvironmentStore.setState({ environments: [host, ubuntu] });
    mocks.connectEnvironment.mockRejectedValue(error);

    await expect(useEnvironmentStore.getState().connect(ubuntu.environment)).rejects.toEqual(error);

    expect(useEnvironmentStore.getState().errorsByEnvironment).toEqual({
      'wsl:ubuntu': error,
    });
    expect(useEnvironmentStore.getState().environments[0]).toEqual(host);
    expect(useEnvironmentStore.getState().environments[1].status).toBe('unavailable');
  });

  it('applies an unavailable runtime event only to the discovered distribution', () => {
    const error: AppError = {
      kind: 'environmentUnavailable',
      data: { environment: ubuntu.environment, message: 'distribution stopped' },
    };
    const event: EnvironmentRuntimeEvent = {
      revision: 2,
      environment: ubuntu.environment,
      status: 'unavailable',
      error,
    };
    useEnvironmentStore.setState({ environments: [host, ubuntu, debian] });

    useEnvironmentStore.getState().applyRuntimeEvent(event);

    expect(useEnvironmentStore.getState().environments).toEqual([
      host,
      { ...ubuntu, status: 'unavailable', revision: 2, error },
      debian,
    ]);
    expect(useEnvironmentStore.getState().errorsByEnvironment).toEqual({
      'wsl:ubuntu': error,
    });
  });

  it('clears only the recovered distribution error on an available runtime event', () => {
    const ubuntuError: AppError = {
      kind: 'environmentUnavailable',
      data: { environment: ubuntu.environment, message: 'distribution stopped' },
    };
    const debianError: AppError = {
      kind: 'environmentUnavailable',
      data: { environment: debian.environment, message: 'distribution stopped' },
    };
    useEnvironmentStore.setState({
      environments: [host, { ...ubuntu, status: 'unavailable' }, debian],
      errorsByEnvironment: {
        'wsl:ubuntu': ubuntuError,
        'wsl:Debian': debianError,
      },
    });

    useEnvironmentStore.getState().applyRuntimeEvent({
      revision: 2,
      environment: ubuntu.environment,
      status: 'available',
      error: null,
    });

    expect(useEnvironmentStore.getState().environments).toEqual([
      host,
      { ...ubuntu, revision: 2 },
      debian,
    ]);
    expect(useEnvironmentStore.getState().errorsByEnvironment).toEqual({
        'wsl:ubuntu': null,
      'wsl:Debian': debianError,
    });
  });

  it('retains runtime events that arrive before a distribution appears in discovery', () => {
    useEnvironmentStore.setState({ environments: [host, ubuntu] });

    useEnvironmentStore.getState().applyRuntimeEvent({
      revision: 2,
      environment: debian.environment,
      status: 'unavailable',
      error: {
        kind: 'environmentUnavailable',
        data: { environment: debian.environment, message: 'distribution stopped' },
      },
    });

    expect(useEnvironmentStore.getState().environments).toEqual([host, ubuntu]);
    expect(useEnvironmentStore.getState().errorsByEnvironment['wsl:debian']?.kind)
      .toBe('environmentUnavailable');
  });
});
