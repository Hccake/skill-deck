/* @vitest-environment jsdom */

import '@/test-utils';
import { act, render } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstallWizardSessionSnapshot } from '@/bindings';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';
import { useInstallWizardSessionMonitor } from '../useInstallWizardSessionMonitor';

const mocks = vi.hoisted(() => ({
  getInstallWizardSession: vi.fn(),
  listen: vi.fn(),
}));

vi.mock('@/bindings', () => ({
  events: {
    installWizardSessionSnapshot: {
      listen: (listener: (event: { payload: InstallWizardSessionSnapshot }) => void) => (
        mocks.listen(listener)
      ),
    },
  },
}));

vi.mock('@/hooks/useTauriApi', () => ({
  getInstallWizardSession: (...args: unknown[]) => mocks.getInstallWizardSession(...args),
}));

function snapshot(revision: number, active: boolean): InstallWizardSessionSnapshot {
  return { revision, active };
}

function MonitorHarness() {
  useInstallWizardSessionMonitor();
  return null;
}

describe('useInstallWizardSessionMonitor', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.listen.mockResolvedValue(vi.fn());
    mocks.getInstallWizardSession.mockResolvedValue(snapshot(0, false));
    useInstallWizardSessionStore.setState({
      revision: 0,
      active: false,
      loading: false,
      syncError: null,
      monitorRetryRevision: 0,
      snapshotVersion: 0,
    });
  });

  it('subscribes before recovery, forwards snapshots, and releases the listener', async () => {
    const unlisten = vi.fn();
    mocks.listen.mockResolvedValue(unlisten);
    const view = render(<MonitorHarness />);

    await vi.waitFor(() => expect(mocks.getInstallWizardSession).toHaveBeenCalledTimes(1));
    expect(mocks.listen.mock.invocationCallOrder[0])
      .toBeLessThan(mocks.getInstallWizardSession.mock.invocationCallOrder[0]);

    const listener = mocks.listen.mock.calls[0][0] as (
      event: { payload: InstallWizardSessionSnapshot },
    ) => void;
    act(() => listener({ payload: snapshot(2, true) }));
    expect(useInstallWizardSessionStore.getState()).toMatchObject({
      revision: 2,
      active: true,
      syncError: null,
    });

    view.unmount();
    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
  });

  it('fails closed when the live session listener cannot be established', async () => {
    const error = new Error('listener unavailable');
    mocks.listen.mockRejectedValue(error);

    render(<MonitorHarness />);

    await vi.waitFor(() => expect(useInstallWizardSessionStore.getState().syncError).toBe('monitor'));
    expect(mocks.getInstallWizardSession).toHaveBeenCalledTimes(1);
    expect(useInstallWizardSessionStore.getState()).toMatchObject({
      active: false,
      loading: false,
      syncError: 'monitor',
    });
  });

  it('re-establishes the listener on focus after monitoring fails', async () => {
    const unlisten = vi.fn();
    mocks.listen
      .mockRejectedValueOnce(new Error('listener unavailable'))
      .mockResolvedValueOnce(unlisten);
    render(<MonitorHarness />);
    await vi.waitFor(() => expect(useInstallWizardSessionStore.getState().syncError).toBe('monitor'));

    act(() => {
      window.dispatchEvent(new Event('focus'));
      window.dispatchEvent(new Event('focus'));
    });

    await vi.waitFor(() => expect(mocks.listen).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(mocks.getInstallWizardSession).toHaveBeenCalledTimes(2));
    expect(useInstallWizardSessionStore.getState().syncError).toBeNull();
  });

  it('recovers the current session again when the main window regains focus', async () => {
    render(<MonitorHarness />);
    await vi.waitFor(() => expect(mocks.getInstallWizardSession).toHaveBeenCalledTimes(1));

    act(() => window.dispatchEvent(new Event('focus')));

    await vi.waitFor(() => expect(mocks.getInstallWizardSession).toHaveBeenCalledTimes(2));
  });

  it('re-establishes monitoring and refreshes the session after retry', async () => {
    const unlisten = vi.fn();
    mocks.listen
      .mockRejectedValueOnce(new Error('listener unavailable'))
      .mockResolvedValueOnce(unlisten);
    mocks.getInstallWizardSession.mockResolvedValue(snapshot(3, true));
    const view = render(<MonitorHarness />);

    await vi.waitFor(() => expect(useInstallWizardSessionStore.getState().syncError).toBe('monitor'));

    act(() => useInstallWizardSessionStore.getState().retryMonitoring());

    await vi.waitFor(() => expect(mocks.listen).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(mocks.getInstallWizardSession).toHaveBeenCalledTimes(2));
    expect(useInstallWizardSessionStore.getState()).toMatchObject({
      revision: 3,
      active: true,
      loading: false,
      syncError: null,
    });

    view.unmount();
    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
  });
});
