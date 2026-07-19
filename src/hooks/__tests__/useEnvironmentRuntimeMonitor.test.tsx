/* @vitest-environment jsdom */

import '@/test-utils';
import { render } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { EnvironmentRuntimeEvent } from '@/bindings';
import { useEnvironmentRuntimeMonitor } from '../useEnvironmentRuntimeMonitor';

const mocks = vi.hoisted(() => ({
  applyRuntimeEvent: vi.fn(),
  discover: vi.fn(),
  listen: vi.fn(),
}));

vi.mock('@/bindings', () => ({
  events: {
    environmentRuntimeEvent: {
      listen: (listener: (event: { payload: EnvironmentRuntimeEvent }) => void) => (
        mocks.listen(listener)
      ),
    },
  },
}));

vi.mock('@/stores/environment', () => ({
  useEnvironmentStore: (selector: (state: unknown) => unknown) => selector({
    applyRuntimeEvent: mocks.applyRuntimeEvent,
    discover: mocks.discover,
  }),
}));

function MonitorHarness() {
  useEnvironmentRuntimeMonitor();
  return null;
}

describe('useEnvironmentRuntimeMonitor', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.discover.mockResolvedValue(undefined);
    mocks.listen.mockResolvedValue(vi.fn());
  });

  it('subscribes before discovery, forwards events, and unsubscribes on unmount', async () => {
    const unlisten = vi.fn();
    const setInterval = vi.spyOn(window, 'setInterval');
    mocks.listen.mockResolvedValue(unlisten);
    const view = render(<MonitorHarness />);
    await vi.waitFor(() => expect(mocks.discover).toHaveBeenCalledTimes(1));
    expect(mocks.listen.mock.invocationCallOrder[0])
      .toBeLessThan(mocks.discover.mock.invocationCallOrder[0]);

    const event: EnvironmentRuntimeEvent = {
      revision: 2,
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      status: 'unavailable',
      error: {
        kind: 'environmentUnavailable',
        data: {
          environment: { kind: 'wsl', distro_name: 'Ubuntu' },
          message: 'distribution stopped',
        },
      },
    };
    const listener = mocks.listen.mock.calls[0][0] as (
      event: { payload: EnvironmentRuntimeEvent },
    ) => void;
    listener({ payload: event });

    expect(mocks.applyRuntimeEvent).toHaveBeenCalledWith(event);
    expect(setInterval).not.toHaveBeenCalled();
    view.unmount();
    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
    setInterval.mockRestore();
  });

  it('refreshes the snapshot on focus without starting a polling timer', async () => {
    const setInterval = vi.spyOn(window, 'setInterval');
    render(<MonitorHarness />);
    await vi.waitFor(() => expect(mocks.discover).toHaveBeenCalledTimes(1));

    window.dispatchEvent(new Event('focus'));

    await vi.waitFor(() => expect(mocks.discover).toHaveBeenCalledTimes(2));
    expect(setInterval).not.toHaveBeenCalled();
    setInterval.mockRestore();
  });

  it('retries listener registration on focus after setup fails', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    mocks.listen
      .mockRejectedValueOnce(new Error('listener unavailable'))
      .mockResolvedValueOnce(vi.fn());
    render(<MonitorHarness />);
    await vi.waitFor(() => expect(mocks.discover).toHaveBeenCalledTimes(1));

    window.dispatchEvent(new Event('focus'));

    await vi.waitFor(() => expect(mocks.listen).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(mocks.discover).toHaveBeenCalledTimes(2));
    expect(consoleError).toHaveBeenCalledTimes(1);
    consoleError.mockRestore();
  });

  it('releases a listener that resolves after unmount', async () => {
    let resolveListen: ((unlisten: () => void) => void) | undefined;
    mocks.listen.mockImplementation(() => new Promise((resolve) => {
      resolveListen = resolve;
    }));
    const unlisten = vi.fn();
    const view = render(<MonitorHarness />);

    view.unmount();
    resolveListen?.(unlisten);

    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
    expect(mocks.applyRuntimeEvent).not.toHaveBeenCalled();
  });
});
