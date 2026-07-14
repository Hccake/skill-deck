/* @vitest-environment jsdom */

import '@/test-utils';
import { useEffect } from 'react';
import { act, render } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useMutationMonitor } from '../useMutationMonitor';

const mocks = vi.hoisted(() => ({
  refreshMutation: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('@/stores/mutation', () => ({
  useMutationStore: (selector: (state: unknown) => unknown) => selector({
    refreshMutation: mocks.refreshMutation,
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
  });

  it('refreshes immediately, on focus, and on the polling interval until unmounted', async () => {
    const onUnmount = vi.fn();
    const view = render(<MonitorHarness onUnmount={onUnmount} />);

    await vi.waitFor(() => expect(mocks.refreshMutation).toHaveBeenCalledTimes(1));

    window.dispatchEvent(new Event('focus'));
    await vi.waitFor(() => expect(mocks.refreshMutation).toHaveBeenCalledTimes(2));

    await act(() => vi.advanceTimersByTimeAsync(2_000));
    expect(mocks.refreshMutation).toHaveBeenCalledTimes(3);

    view.unmount();
    expect(onUnmount).toHaveBeenCalledTimes(1);

    await act(() => vi.advanceTimersByTimeAsync(2_000));
    expect(mocks.refreshMutation).toHaveBeenCalledTimes(3);

    vi.useRealTimers();
  });
});
