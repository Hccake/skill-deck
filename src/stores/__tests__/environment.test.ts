import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
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

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

describe('useEnvironmentStore', () => {
  let now: number;

  beforeEach(() => {
    vi.clearAllMocks();
    now = 1_000_000;
    vi.spyOn(Date, 'now').mockImplementation(() => now);
    useEnvironmentStore.setState({
      environments: [],
      runtimeByEnvironment: {},
      discoveryState: 'idle',
      discoveryError: null,
      errorsByEnvironment: {},
      discoveryCompletedAt: null,
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
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

  it('keeps Host available when the initial discovery request rejects', async () => {
    const error: AppError = {
      kind: 'environmentDiscoveryFailed',
      data: { message: 'listEnvironments IPC failed' },
    };
    mocks.listEnvironments.mockRejectedValue(error);

    await expect(useEnvironmentStore.getState().discover()).rejects.toEqual(error);

    expect(useEnvironmentStore.getState()).toMatchObject({
      environments: [{
        environment: { kind: 'host' },
        displayName: 'Host',
        status: 'available',
        revision: 0,
        error: null,
      }],
      discoveryState: 'error',
      discoveryError: error,
    });
  });

  it('shares one in-flight request across callers', async () => {
    const request = deferred<{ environments: EnvironmentInfo[]; error: AppError | null }>();
    mocks.listEnvironments.mockReturnValue(request.promise);

    const initial = useEnvironmentStore.getState().discover();
    const resume = useEnvironmentStore.getState().discover();
    expect(resume).toBe(initial);
    expect(mocks.listEnvironments).toHaveBeenCalledTimes(1);

    request.resolve({ environments: [host, ubuntu], error: null });
    await initial;
  });

  it('suppresses automatic discovery for 30 seconds after an attempt completes', async () => {
    mocks.listEnvironments.mockResolvedValue({ environments: [host, ubuntu], error: null });

    await useEnvironmentStore.getState().discover();
    now += 29_999;
    await useEnvironmentStore.getState().discover();
    expect(mocks.listEnvironments).toHaveBeenCalledTimes(1);

    now += 1;
    await useEnvironmentStore.getState().discover();
    expect(mocks.listEnvironments).toHaveBeenCalledTimes(2);
  });

  it('keeps the last successful inventory when discovery returns a typed error', async () => {
    const error: AppError = {
      kind: 'environmentDiscoveryFailed',
      data: { message: 'wsl.exe is blocked' },
    };
    mocks.listEnvironments
      .mockResolvedValueOnce({ environments: [host, ubuntu], error: null })
      .mockResolvedValueOnce({ environments: [host], error });

    await useEnvironmentStore.getState().discover();
    now += 30_000;
    await useEnvironmentStore.getState().discover();

    expect(useEnvironmentStore.getState()).toMatchObject({
      environments: [host, ubuntu],
      discoveryState: 'error',
      discoveryError: error,
    });

    now += 29_999;
    await useEnvironmentStore.getState().discover();
    expect(mocks.listEnvironments).toHaveBeenCalledTimes(2);
  });

  it('keeps the last successful inventory when discovery rejects', async () => {
    const error: AppError = {
      kind: 'environmentDiscoveryFailed',
      data: { message: 'wsl.exe timed out' },
    };
    mocks.listEnvironments
      .mockResolvedValueOnce({ environments: [host, ubuntu], error: null })
      .mockRejectedValueOnce(error);

    await useEnvironmentStore.getState().discover();
    now += 30_000;
    await expect(useEnvironmentStore.getState().discover()).rejects.toEqual(error);

    expect(useEnvironmentStore.getState()).toMatchObject({
      environments: [host, ubuntu],
      discoveryState: 'error',
      discoveryError: error,
    });
  });

  it('keeps the current inventory visible while replacing it after a successful refresh', async () => {
    mocks.listEnvironments.mockResolvedValueOnce({
      environments: [host, ubuntu, debian],
      error: null,
    });
    await useEnvironmentStore.getState().discover();

    const request = deferred<{ environments: EnvironmentInfo[]; error: AppError | null }>();
    mocks.listEnvironments.mockReturnValueOnce(request.promise);
    now += 30_000;
    const refresh = useEnvironmentStore.getState().discover();

    expect(useEnvironmentStore.getState()).toMatchObject({
      environments: [host, ubuntu, debian],
      discoveryState: 'ready',
    });

    request.resolve({ environments: [host, debian], error: null });
    await refresh;

    expect(useEnvironmentStore.getState()).toMatchObject({
      environments: [host, debian],
      discoveryState: 'ready',
      discoveryError: null,
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
