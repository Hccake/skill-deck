/* @vitest-environment jsdom */

import '@/test-utils';
import { useEffect } from 'react';
import { act, render } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useMutationMonitor } from '../useMutationMonitor';

const mocks = vi.hoisted(() => ({
  refreshMutation: vi.fn().mockResolvedValue(undefined),
  acceptSnapshot: vi.fn(),
  listen: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: (...args: unknown[]) => mocks.listen(...args),
}));

vi.mock('@/stores/mutation', () => ({
  useMutationStore: (selector: (state: unknown) => unknown) => selector({
    refreshMutation: mocks.refreshMutation,
    acceptSnapshot: mocks.acceptSnapshot,
  }),
}));

function MonitorHarness({ onUnmount }: { onUnmount: () => void }) {
  useMutationMonitor(2_000);

  useEffect(() => onUnmount, [onUnmount]);
  return null;
}

describe('useMutationMonitor', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    mocks.listen.mockResolvedValue(vi.fn());
  });

  it('registers the event listener before the startup refresh and never polls', async () => {
    let resolveListen: ((unlisten: () => void) => void) | undefined;
    mocks.listen.mockImplementation(() => new Promise((resolve) => {
      resolveListen = resolve;
    }));
    const onUnmount = vi.fn();
    const view = render(<MonitorHarness onUnmount={onUnmount} />);

    expect(mocks.refreshMutation).not.toHaveBeenCalled();
    resolveListen?.(vi.fn());
    await vi.waitFor(() => expect(mocks.refreshMutation).toHaveBeenCalledTimes(1));

    window.dispatchEvent(new Event('focus'));
    await vi.waitFor(() => expect(mocks.refreshMutation).toHaveBeenCalledTimes(2));

    await act(() => vi.advanceTimersByTimeAsync(2_000));
    expect(mocks.refreshMutation).toHaveBeenCalledTimes(2);

    view.unmount();
    expect(onUnmount).toHaveBeenCalledTimes(1);

    await act(() => vi.advanceTimersByTimeAsync(2_000));
    expect(mocks.refreshMutation).toHaveBeenCalledTimes(2);

    vi.useRealTimers();
  });

  it('applies backend mutation snapshot events and unsubscribes on unmount', async () => {
    const unlisten = vi.fn();
    mocks.listen.mockResolvedValue(unlisten);
    const view = render(<MonitorHarness onUnmount={vi.fn()} />);
    await vi.waitFor(() => expect(mocks.listen).toHaveBeenCalledTimes(1));

    const listener = mocks.listen.mock.calls[0][1] as (event: { payload: unknown }) => void;
    listener({ payload: { revision: 2, active: null } });

    expect(mocks.acceptSnapshot).toHaveBeenCalledWith({ revision: 2, active: null });
    view.unmount();
    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
  });

  it('performs the startup refresh when listener registration fails', async () => {
    const error = new Error('listener unavailable');
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    mocks.listen.mockRejectedValue(error);
    const view = render(<MonitorHarness onUnmount={vi.fn()} />);

    await vi.waitFor(() => expect(mocks.refreshMutation).toHaveBeenCalledTimes(1));
    expect(consoleError).toHaveBeenCalledWith('Failed to monitor mutation state:', error);

    window.dispatchEvent(new Event('focus'));
    await vi.waitFor(() => expect(mocks.refreshMutation).toHaveBeenCalledTimes(2));

    view.unmount();
    consoleError.mockRestore();
  });

  it('releases a listener that resolves after unmount without refreshing', async () => {
    let resolveListen: ((unlisten: () => void) => void) | undefined;
    mocks.listen.mockImplementation(() => new Promise((resolve) => {
      resolveListen = resolve;
    }));
    const unlisten = vi.fn();
    const view = render(<MonitorHarness onUnmount={vi.fn()} />);

    view.unmount();
    resolveListen?.(unlisten);

    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
    expect(mocks.refreshMutation).not.toHaveBeenCalled();
  });
});
