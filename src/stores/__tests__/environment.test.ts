import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppError, EnvironmentInfo, EnvironmentRuntimeEvent } from '@/bindings';
import { useEnvironmentStore } from '../environment';
import { useInstallWizardSessionStore } from '../install-wizard-session';

const mocks = vi.hoisted(() => ({
  listEnvironments: vi.fn(),
  connectEnvironment: vi.fn(),
  setWslIntegrationEnabled: vi.fn(),
  getInstallWizardSession: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  listEnvironments: (...args: unknown[]) => mocks.listEnvironments(...args),
  connectEnvironment: (...args: unknown[]) => mocks.connectEnvironment(...args),
  setWslIntegrationEnabled: (...args: unknown[]) => mocks.setWslIntegrationEnabled(...args),
  getInstallWizardSession: () => mocks.getInstallWizardSession(),
}));

const native: EnvironmentInfo = {
  environment: { kind: 'native' },
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
    mocks.connectEnvironment.mockResolvedValue(ubuntu);
    useEnvironmentStore.setState({
      environments: [],
      runtimeByEnvironment: {},
      discoveryState: 'idle',
      discoveryError: null,
      discoveryCompletedAt: null,
      wslIntegrationSupported: false,
      wslIntegrationEnabled: false,
    });
    useInstallWizardSessionStore.setState({
      revision: 0, active: false, loading: false, hasConfirmedSnapshot: false,
      syncError: null, monitorRetryRevision: 0, snapshotVersion: 0,
    });
    mocks.getInstallWizardSession.mockResolvedValue({ revision: 1, active: true });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('discovers environments without connecting a distribution', async () => {
    mocks.listEnvironments.mockResolvedValue({
      environments: [native, ubuntu],
      error: null,
      wslIntegrationSupported: true,
      wslIntegrationEnabled: true,
    });

    await useEnvironmentStore.getState().discover();

    expect(useEnvironmentStore.getState().environments).toEqual([native, ubuntu]);
    expect(useEnvironmentStore.getState().discoveryState).toBe('ready');
    expect(useEnvironmentStore.getState().wslIntegrationSupported).toBe(true);
    expect(useEnvironmentStore.getState().wslIntegrationEnabled).toBe(true);
    expect(mocks.connectEnvironment).not.toHaveBeenCalled();
  });

  it('does not let an older connection response overwrite a newer runtime event', async () => {
    const connection = deferred<EnvironmentInfo>();
    mocks.connectEnvironment.mockReturnValue(connection.promise);
    useEnvironmentStore.setState({
      environments: [native, ubuntu],
      runtimeByEnvironment: { native, 'wsl:ubuntu': ubuntu },
      wslIntegrationEnabled: true,
      wslCapabilityRevision: 4,
    });

    const pending = useEnvironmentStore.getState().connect(ubuntu.environment);
    useEnvironmentStore.getState().applyRuntimeEvent({
      environment: ubuntu.environment,
      status: 'unavailable',
      revision: 3,
      capabilityRevision: 4,
      error: { kind: 'environmentUnavailable', data: { environment: ubuntu.environment, message: 'stopped' } },
    });
    connection.resolve({ ...ubuntu, status: 'available', revision: 2 });
    await pending;

    expect(useEnvironmentStore.getState()).toMatchObject({
      environments: [native, {
        environment: ubuntu.environment,
        status: 'unavailable',
        revision: 3,
      }],
      runtimeByEnvironment: {
        native,
        'wsl:ubuntu': {
          environment: ubuntu.environment,
          status: 'unavailable',
          revision: 3,
        },
      },
    });
  });

  it('does not let an older connection failure overwrite a newer runtime event', async () => {
    const connection = deferred<EnvironmentInfo>();
    mocks.connectEnvironment.mockReturnValue(connection.promise);
    useEnvironmentStore.setState({
      environments: [native, ubuntu],
      runtimeByEnvironment: { native, 'wsl:ubuntu': ubuntu },
      wslIntegrationEnabled: true,
      wslCapabilityRevision: 4,
    });

    const pending = useEnvironmentStore.getState().connect(ubuntu.environment);
    useEnvironmentStore.getState().applyRuntimeEvent({
      environment: ubuntu.environment,
      status: 'available',
      revision: 3,
      capabilityRevision: 4,
      error: null,
    });
    connection.reject({
      kind: 'environmentUnavailable',
      data: { environment: ubuntu.environment, message: 'older failure' },
    });
    await expect(pending).rejects.toMatchObject({ kind: 'environmentUnavailable' });

    expect(useEnvironmentStore.getState()).toMatchObject({
      environments: [native, {
        environment: ubuntu.environment,
        status: 'available',
        revision: 3,
        error: null,
      }],
      runtimeByEnvironment: {
        native,
        'wsl:ubuntu': {
          environment: ubuntu.environment,
          status: 'available',
          revision: 3,
          error: null,
        },
      },
    });
  });

  it('applies the authoritative Native-only snapshot after disabling WSL integration', async () => {
    useEnvironmentStore.setState({
      environments: [native, ubuntu],
      runtimeByEnvironment: { native, 'wsl:ubuntu': ubuntu },
      wslIntegrationSupported: true,
      wslIntegrationEnabled: true,
    });
    mocks.setWslIntegrationEnabled.mockResolvedValue({
      environments: [native],
      error: null,
      wslIntegrationSupported: true,
      wslIntegrationEnabled: false,
    });

    await useEnvironmentStore.getState().setWslIntegrationEnabled(false);

    expect(mocks.setWslIntegrationEnabled).toHaveBeenCalledWith(false);
    expect(useEnvironmentStore.getState()).toMatchObject({
      environments: [native],
      runtimeByEnvironment: { native },
      discoveryState: 'ready',
      discoveryError: null,
      wslIntegrationSupported: true,
      wslIntegrationEnabled: false,
    });
  });

  it('keeps the current snapshot when installation wins WSL-setting admission', async () => {
    useEnvironmentStore.setState({
      environments: [native, ubuntu],
      runtimeByEnvironment: { native, 'wsl:ubuntu': ubuntu },
      wslIntegrationSupported: true,
      wslIntegrationEnabled: true,
    });
    mocks.setWslIntegrationEnabled.mockRejectedValue({ kind: 'installWizardActive' });

    const changed = await useEnvironmentStore.getState().setWslIntegrationEnabled(false);

    expect(changed).toBe(false);
    expect(useEnvironmentStore.getState()).toMatchObject({
      environments: [native, ubuntu],
      wslIntegrationEnabled: true,
    });
  });

  it('does not let an older discovery overwrite a completed WSL setting change', async () => {
    const discovery = deferred<{
      environments: EnvironmentInfo[];
      error: AppError | null;
      wslIntegrationSupported: boolean;
      wslIntegrationEnabled: boolean;
    }>();
    mocks.listEnvironments.mockReturnValue(discovery.promise);
    mocks.setWslIntegrationEnabled.mockResolvedValue({
      environments: [native],
      error: null,
      wslIntegrationSupported: true,
      wslIntegrationEnabled: false,
    });

    const pendingDiscovery = useEnvironmentStore.getState().discover();
    await useEnvironmentStore.getState().setWslIntegrationEnabled(false);
    discovery.resolve({
      environments: [native, ubuntu],
      error: null,
      wslIntegrationSupported: true,
      wslIntegrationEnabled: true,
    });
    await pendingDiscovery;

    expect(useEnvironmentStore.getState()).toMatchObject({
      environments: [native],
      runtimeByEnvironment: { native },
      wslIntegrationEnabled: false,
    });
  });

  it('does not start discovery while a WSL setting change is pending', async () => {
    const setting = deferred<{
      environments: EnvironmentInfo[];
      error: AppError | null;
      wslIntegrationSupported: boolean;
      wslIntegrationEnabled: boolean;
    }>();
    mocks.setWslIntegrationEnabled.mockReturnValue(setting.promise);
    mocks.listEnvironments.mockResolvedValue({
      environments: [native],
      error: null,
      wslIntegrationSupported: true,
      wslIntegrationEnabled: false,
    });

    const pendingSetting = useEnvironmentStore.getState().setWslIntegrationEnabled(true);
    const pendingDiscovery = useEnvironmentStore.getState().discover();

    expect(mocks.listEnvironments).not.toHaveBeenCalled();
    setting.resolve({
      environments: [native, ubuntu],
      error: null,
      wslIntegrationSupported: true,
      wslIntegrationEnabled: true,
    });
    await Promise.all([pendingSetting, pendingDiscovery]);
    expect(useEnvironmentStore.getState().wslIntegrationEnabled).toBe(true);
  });

  it('keeps Native usable and exposes a typed discovery error', async () => {
    const error: AppError = {
      kind: 'environmentDiscoveryFailed',
      data: { message: 'wsl.exe is blocked' },
    };
    mocks.listEnvironments.mockResolvedValue({ environments: [native], error });

    await useEnvironmentStore.getState().discover();

    expect(useEnvironmentStore.getState()).toMatchObject({
      environments: [native],
      discoveryState: 'error',
      discoveryError: error,
    });
  });

  it('keeps Native available when the initial discovery request rejects', async () => {
    const error: AppError = {
      kind: 'environmentDiscoveryFailed',
      data: { message: 'listEnvironments IPC failed' },
    };
    mocks.listEnvironments.mockRejectedValue(error);

    await expect(useEnvironmentStore.getState().discover()).rejects.toEqual(error);

    expect(useEnvironmentStore.getState()).toMatchObject({
      environments: [{
        environment: { kind: 'native' },
        displayName: 'Native',
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

    request.resolve({ environments: [native, ubuntu], error: null });
    await initial;
  });

  it('suppresses automatic discovery for 30 seconds after an attempt completes', async () => {
    mocks.listEnvironments.mockResolvedValue({ environments: [native, ubuntu], error: null });

    await useEnvironmentStore.getState().discover();
    now += 29_999;
    await useEnvironmentStore.getState().discover();
    expect(mocks.listEnvironments).toHaveBeenCalledTimes(1);

    now += 1;
    await useEnvironmentStore.getState().discover();
    expect(mocks.listEnvironments).toHaveBeenCalledTimes(2);
  });

  it('lets an explicit retry bypass the automatic discovery cooldown', async () => {
    const error: AppError = {
      kind: 'environmentDiscoveryFailed',
      data: { message: 'wsl.exe is blocked' },
    };
    mocks.listEnvironments
      .mockResolvedValueOnce({ environments: [native], error })
      .mockResolvedValueOnce({ environments: [native, ubuntu], error: null });

    await useEnvironmentStore.getState().discover();
    await useEnvironmentStore.getState().retryDiscovery();

    expect(mocks.listEnvironments).toHaveBeenCalledTimes(2);
    expect(useEnvironmentStore.getState()).toMatchObject({
      environments: [native, ubuntu],
      discoveryState: 'ready',
      discoveryError: null,
    });
  });

  it('keeps the last successful inventory when discovery returns a typed error', async () => {
    const error: AppError = {
      kind: 'environmentDiscoveryFailed',
      data: { message: 'wsl.exe is blocked' },
    };
    mocks.listEnvironments
      .mockResolvedValueOnce({ environments: [native, ubuntu], error: null })
      .mockResolvedValueOnce({ environments: [native], error });

    await useEnvironmentStore.getState().discover();
    now += 30_000;
    await useEnvironmentStore.getState().discover();

    expect(useEnvironmentStore.getState()).toMatchObject({
      environments: [native, ubuntu],
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
      .mockResolvedValueOnce({ environments: [native, ubuntu], error: null })
      .mockRejectedValueOnce(error);

    await useEnvironmentStore.getState().discover();
    now += 30_000;
    await expect(useEnvironmentStore.getState().discover()).rejects.toEqual(error);

    expect(useEnvironmentStore.getState()).toMatchObject({
      environments: [native, ubuntu],
      discoveryState: 'error',
      discoveryError: error,
    });
  });

  it('keeps the current inventory visible while replacing it after a successful refresh', async () => {
    mocks.listEnvironments.mockResolvedValueOnce({
      environments: [native, ubuntu, debian],
      error: null,
    });
    await useEnvironmentStore.getState().discover();

    const request = deferred<{ environments: EnvironmentInfo[]; error: AppError | null }>();
    mocks.listEnvironments.mockReturnValueOnce(request.promise);
    now += 30_000;
    const refresh = useEnvironmentStore.getState().discover();

    expect(useEnvironmentStore.getState()).toMatchObject({
      environments: [native, ubuntu, debian],
      discoveryState: 'ready',
    });

    request.resolve({ environments: [native, debian], error: null });
    await refresh;

    expect(useEnvironmentStore.getState()).toMatchObject({
      environments: [native, debian],
      discoveryState: 'ready',
      discoveryError: null,
    });
  });

  it('treats Native connection as immediately available', async () => {
    useEnvironmentStore.setState({ environments: [native, ubuntu] });

    await useEnvironmentStore.getState().connect(native.environment);

    expect(mocks.connectEnvironment).not.toHaveBeenCalled();
    expect(useEnvironmentStore.getState().environments[0].status).toBe('available');
  });

  it('exposes WSL connecting state without owning selected context', async () => {
    let finishConnect: ((environment: EnvironmentInfo) => void) | undefined;
    useEnvironmentStore.setState({ environments: [native, ubuntu] });
    mocks.connectEnvironment.mockImplementation(() => new Promise<EnvironmentInfo>((resolve) => {
      finishConnect = resolve;
    }));

    const connection = useEnvironmentStore.getState().connect(ubuntu.environment);

    expect(useEnvironmentStore.getState().environments[1].status).toBe('connecting');
    expect('selectedEnvironment' in useEnvironmentStore.getState()).toBe(false);
    finishConnect?.(ubuntu);
    await connection;
    expect(useEnvironmentStore.getState().environments[1].status).toBe('available');
  });

  it('applies the authoritative EnvironmentInfo returned by WSL connection', async () => {
    const connected = { ...ubuntu, revision: 7 };
    useEnvironmentStore.setState({ environments: [native, ubuntu] });
    mocks.connectEnvironment.mockResolvedValue(connected);

    await useEnvironmentStore.getState().connect(ubuntu.environment);

    expect(useEnvironmentStore.getState().environments[1]).toEqual(connected);
  });

  it('stores a typed connection error only on the failed EnvironmentInfo', async () => {
    const error: AppError = {
      kind: 'environmentUnavailable',
      data: { environment: ubuntu.environment, message: 'distribution stopped' },
    };
    useEnvironmentStore.setState({ environments: [native, ubuntu] });
    mocks.connectEnvironment.mockRejectedValue(error);

    await expect(useEnvironmentStore.getState().connect(ubuntu.environment)).rejects.toEqual(error);

    expect('errorsByEnvironment' in useEnvironmentStore.getState()).toBe(false);
    expect(useEnvironmentStore.getState().environments[0]).toEqual(native);
    expect(useEnvironmentStore.getState().environments[1]).toEqual({
      ...ubuntu,
      status: 'unavailable',
      error,
    });
  });

  it('applies an unavailable runtime event only to the discovered distribution', () => {
    const error: AppError = {
      kind: 'environmentUnavailable',
      data: { environment: ubuntu.environment, message: 'distribution stopped' },
    };
    const event: EnvironmentRuntimeEvent = {
      capabilityRevision: 0,
      revision: 2,
      environment: ubuntu.environment,
      status: 'unavailable',
      error,
    };
    useEnvironmentStore.setState({
      environments: [native, ubuntu, debian],
      wslIntegrationEnabled: true,
    });

    useEnvironmentStore.getState().applyRuntimeEvent(event);

    expect(useEnvironmentStore.getState().environments).toEqual([
      native,
      { ...ubuntu, status: 'unavailable', revision: 2, error },
      debian,
    ]);
    expect('errorsByEnvironment' in useEnvironmentStore.getState()).toBe(false);
  });

  it('clears only the recovered EnvironmentInfo error on an available runtime event', () => {
    const ubuntuError: AppError = {
      kind: 'environmentUnavailable',
      data: { environment: ubuntu.environment, message: 'distribution stopped' },
    };
    const debianError: AppError = {
      kind: 'environmentUnavailable',
      data: { environment: debian.environment, message: 'distribution stopped' },
    };
    useEnvironmentStore.setState({
      environments: [
        native,
        { ...ubuntu, status: 'unavailable', error: ubuntuError },
        { ...debian, status: 'unavailable', error: debianError },
      ],
      wslIntegrationEnabled: true,
    });

    useEnvironmentStore.getState().applyRuntimeEvent({
      capabilityRevision: 0,
      revision: 2,
      environment: ubuntu.environment,
      status: 'available',
      error: null,
    });

    expect(useEnvironmentStore.getState().environments).toEqual([
      native,
      { ...ubuntu, revision: 2 },
      { ...debian, status: 'unavailable', error: debianError },
    ]);
    expect('errorsByEnvironment' in useEnvironmentStore.getState()).toBe(false);
  });

  it('retains runtime events that arrive before a distribution appears in discovery', () => {
    useEnvironmentStore.setState({
      environments: [native, ubuntu],
      wslIntegrationEnabled: true,
    });

    useEnvironmentStore.getState().applyRuntimeEvent({
      capabilityRevision: 0,
      revision: 2,
      environment: debian.environment,
      status: 'unavailable',
      error: {
        kind: 'environmentUnavailable',
        data: { environment: debian.environment, message: 'distribution stopped' },
      },
    });

    expect(useEnvironmentStore.getState().environments).toEqual([native, ubuntu]);
    expect(useEnvironmentStore.getState().runtimeByEnvironment['wsl:debian']?.error?.kind)
      .toBe('environmentUnavailable');
  });
});
