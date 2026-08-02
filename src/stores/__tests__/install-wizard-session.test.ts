import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstallWizardSessionSnapshot } from '@/bindings';
import {
  prepareInstallWizardSessionMonitoring,
  useInstallWizardSessionStore,
} from '../install-wizard-session';

const mocks = vi.hoisted(() => ({
  getInstallWizardSession: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  getInstallWizardSession: (...args: unknown[]) => mocks.getInstallWizardSession(...args),
}));

function snapshot(revision: number, active: boolean): InstallWizardSessionSnapshot {
  return { revision, active };
}

describe('useInstallWizardSessionStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useInstallWizardSessionStore.setState({
      revision: 0,
      active: false,
      loading: false,
      syncError: null,
      monitorRetryRevision: 0,
    });
  });

  it('accepts an initial inactive snapshot without requiring a revision change', async () => {
    useInstallWizardSessionStore.setState({ loading: true, syncError: 'refresh' });
    mocks.getInstallWizardSession.mockResolvedValue(snapshot(0, false));

    await useInstallWizardSessionStore.getState().refreshSession();

    expect(useInstallWizardSessionStore.getState()).toMatchObject({
      revision: 0,
      active: false,
      loading: false,
      syncError: null,
    });
  });

  it('starts fail-closed monitoring before the main window renders only', () => {
    prepareInstallWizardSessionMonitoring('/');
    expect(useInstallWizardSessionStore.getState().loading).toBe(true);

    useInstallWizardSessionStore.setState({ loading: false });
    prepareInstallWizardSessionMonitoring('/wizard');
    expect(useInstallWizardSessionStore.getState().loading).toBe(false);
  });

  it('recovers an active wizard session from the backend snapshot', async () => {
    mocks.getInstallWizardSession.mockResolvedValue(snapshot(1, true));

    await useInstallWizardSessionStore.getState().refreshSession();

    expect(useInstallWizardSessionStore.getState().active).toBe(true);
  });

  it('shares one backend request between overlapping refreshes', async () => {
    let resolveSnapshot: ((value: InstallWizardSessionSnapshot) => void) | undefined;
    mocks.getInstallWizardSession.mockImplementation(() => new Promise((resolve) => {
      resolveSnapshot = resolve;
    }));

    const first = useInstallWizardSessionStore.getState().refreshSession();
    const second = useInstallWizardSessionStore.getState().refreshSession();

    expect(mocks.getInstallWizardSession).toHaveBeenCalledTimes(1);
    resolveSnapshot?.(snapshot(1, true));
    await Promise.all([first, second]);

    expect(useInstallWizardSessionStore.getState().active).toBe(true);
    expect(useInstallWizardSessionStore.getState().loading).toBe(false);
  });

  it('ignores an older query after a newer close event', async () => {
    useInstallWizardSessionStore.setState({ revision: 4, active: false });
    mocks.getInstallWizardSession.mockResolvedValue(snapshot(3, true));

    await useInstallWizardSessionStore.getState().refreshSession();

    expect(useInstallWizardSessionStore.getState()).toMatchObject({
      revision: 4,
      active: false,
    });
  });

  it('fails closed when the backend session query cannot be completed', async () => {
    mocks.getInstallWizardSession.mockRejectedValue(new Error('backend unavailable'));

    await expect(
      useInstallWizardSessionStore.getState().refreshSession(),
    ).rejects.toThrow('backend unavailable');

    expect(useInstallWizardSessionStore.getState()).toMatchObject({
      loading: false,
      syncError: 'refresh',
    });
  });

  it('keeps a monitor failure fail-closed after an inactive backend snapshot', async () => {
    useInstallWizardSessionStore.setState({ syncError: 'monitor' });
    mocks.getInstallWizardSession.mockResolvedValue(snapshot(3, false));

    await useInstallWizardSessionStore.getState().refreshSession({
      preserveMonitorError: true,
    });

    expect(useInstallWizardSessionStore.getState()).toMatchObject({
      revision: 3,
      active: false,
      loading: false,
      syncError: 'monitor',
    });
  });

  it('keeps a newer event authoritative when an overlapping query fails', async () => {
    let rejectQuery: ((error: Error) => void) | undefined;
    mocks.getInstallWizardSession.mockImplementation(() => new Promise((_, reject) => {
      rejectQuery = reject;
    }));

    const refresh = useInstallWizardSessionStore.getState().refreshSession();
    useInstallWizardSessionStore.getState().acceptSnapshot(snapshot(1, true));
    rejectQuery?.(new Error('stale query failed'));

    await expect(refresh).rejects.toThrow('stale query failed');
    expect(useInstallWizardSessionStore.getState()).toMatchObject({
      revision: 1,
      active: true,
      loading: false,
      syncError: null,
    });
  });
});
