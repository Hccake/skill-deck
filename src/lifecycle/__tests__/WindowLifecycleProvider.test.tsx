/* @vitest-environment jsdom */

import '@/test-utils';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ActiveMutation, LifecycleAction } from '@/bindings';
import { useMutationStore } from '@/stores/mutation';
import { WindowLifecycleProvider } from '../WindowLifecycleProvider';
import { useWindowLifecycle } from '../useWindowLifecycle';

const mocks = vi.hoisted(() => ({
  windowLabel: 'main',
  closeHandler: undefined as undefined | ((event: { preventDefault: () => void }) => Promise<void>),
  lifecycleHandler: undefined as undefined | ((event: {
    payload: { action: 'closeCurrentWindow' | 'quitApplication' | 'restartApplication' };
  }) => void),
  executeLifecycleAction: vi.fn(),
  onCloseRequested: vi.fn(),
  closeUnlisten: vi.fn(),
  lifecycleUnlisten: vi.fn(),
  listenLifecycle: vi.fn(),
  cancelActiveMutation: vi.fn(),
}));

const mutation: ActiveMutation = {
  id: 'mutation-1',
  kind: 'install',
  context: {
    environment: { kind: 'wsl', distro_name: 'Ubuntu' },
    scope: { scope: 'global' },
  },
  phase: 'materializing',
  progress: null,
  cancelable: true,
};

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({
    label: mocks.windowLabel,
    onCloseRequested: mocks.onCloseRequested,
  }),
}));

vi.mock('@/bindings', () => ({
  events: {
    lifecycleActionRequestedEvent: {
      listen: mocks.listenLifecycle,
    },
  },
}));

vi.mock('../lifecycleApi', () => ({
  executeLifecycleAction: (...args: unknown[]) => mocks.executeLifecycleAction(...args),
}));

vi.mock('@/components/layout/MutationInterruptionDialog', () => ({
  MutationInterruptionDialog: ({
    open,
    action,
    cancelable,
    onContinueWaiting,
    onCancelAndContinue,
  }: {
    open: boolean;
    action: LifecycleAction;
    cancelable: boolean;
    onContinueWaiting: () => void;
    onCancelAndContinue: () => void;
  }) => open ? (
    <div>
      <span>{`dialog-${action}`}</span>
      <button type="button" onClick={onContinueWaiting}>continue-waiting</button>
      {cancelable ? (
        <button type="button" onClick={onCancelAndContinue}>cancel-and-continue</button>
      ) : null}
    </div>
  ) : null,
}));

function LifecycleHarness() {
  const { requestAction } = useWindowLifecycle();
  return (
    <button type="button" onClick={() => void requestAction('restartApplication')}>
      restart
    </button>
  );
}

async function requestNativeClose() {
  const preventDefault = vi.fn();
  await act(async () => {
    await mocks.closeHandler?.({ preventDefault });
  });
  return preventDefault;
}

describe('WindowLifecycleProvider', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.windowLabel = 'main';
    mocks.closeHandler = undefined;
    mocks.lifecycleHandler = undefined;
    mocks.executeLifecycleAction.mockResolvedValue({ status: 'performed' });
    mocks.cancelActiveMutation.mockResolvedValue(true);
    mocks.onCloseRequested.mockImplementation(async (handler) => {
      mocks.closeHandler = handler;
      return mocks.closeUnlisten;
    });
    mocks.listenLifecycle.mockImplementation(async (handler) => {
      mocks.lifecycleHandler = handler;
      return mocks.lifecycleUnlisten;
    });
    useMutationStore.setState({
      revision: 0,
      activeMutation: null,
      loading: false,
      cancelling: false,
      cancelActiveMutation: mocks.cancelActiveMutation,
    });
  });

  it('maps the main window close request to application quit', async () => {
    render(
      <WindowLifecycleProvider>
        <LifecycleHarness />
      </WindowLifecycleProvider>,
    );
    await waitFor(() => expect(mocks.closeHandler).toBeDefined());

    const preventDefault = await requestNativeClose();

    expect(preventDefault).toHaveBeenCalledTimes(1);
    expect(mocks.executeLifecycleAction).toHaveBeenCalledWith('quitApplication');
  });

  it('maps the install wizard close request to closing only that window', async () => {
    mocks.windowLabel = 'install-wizard';
    render(
      <WindowLifecycleProvider>
        <LifecycleHarness />
      </WindowLifecycleProvider>,
    );
    await waitFor(() => expect(mocks.closeHandler).toBeDefined());

    await requestNativeClose();

    expect(mocks.executeLifecycleAction).toHaveBeenCalledWith('closeCurrentWindow');
  });

  it('executes a delegated lifecycle action in the receiving window', async () => {
    render(
      <WindowLifecycleProvider>
        <LifecycleHarness />
      </WindowLifecycleProvider>,
    );
    await waitFor(() => expect(mocks.lifecycleHandler).toBeDefined());

    act(() => {
      mocks.lifecycleHandler?.({ payload: { action: 'restartApplication' } });
    });

    await waitFor(() => {
      expect(mocks.executeLifecycleAction).toHaveBeenCalledWith('restartApplication');
    });
  });

  it('exposes programmatic lifecycle requests through one window context', async () => {
    render(
      <WindowLifecycleProvider>
        <LifecycleHarness />
      </WindowLifecycleProvider>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'restart' }));

    await waitFor(() => {
      expect(mocks.executeLifecycleAction).toHaveBeenCalledWith('restartApplication');
    });
  });

  it('shows the matching interruption action when Rust reports a mutation block', async () => {
    mocks.executeLifecycleAction.mockImplementation(async (action: LifecycleAction) => {
      useMutationStore.setState({ revision: 1, activeMutation: mutation });
      return {
        status: 'blocked',
        snapshot: { revision: 1, active: mutation },
        action,
      };
    });
    render(
      <WindowLifecycleProvider>
        <LifecycleHarness />
      </WindowLifecycleProvider>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'restart' }));

    expect(await screen.findByText('dialog-restartApplication')).toBeDefined();
  });

  it('clears the pending intent when the user continues waiting', async () => {
    mocks.executeLifecycleAction.mockImplementation(async () => {
      useMutationStore.setState({ revision: 1, activeMutation: mutation });
      return { status: 'blocked', snapshot: { revision: 1, active: mutation } };
    });
    render(
      <WindowLifecycleProvider>
        <LifecycleHarness />
      </WindowLifecycleProvider>,
    );
    fireEvent.click(screen.getByRole('button', { name: 'restart' }));
    await screen.findByText('dialog-restartApplication');

    fireEvent.click(screen.getByRole('button', { name: 'continue-waiting' }));
    act(() => {
      useMutationStore.setState({ revision: 2, activeMutation: null });
    });

    await waitFor(() => expect(screen.queryByText('dialog-restartApplication')).toBeNull());
    expect(mocks.executeLifecycleAction).toHaveBeenCalledTimes(1);
  });

  it('retries the original action only after cancellation fully clears the mutation', async () => {
    mocks.executeLifecycleAction
      .mockImplementationOnce(async () => {
        useMutationStore.setState({ revision: 1, activeMutation: mutation });
        return { status: 'blocked', snapshot: { revision: 1, active: mutation } };
      })
      .mockResolvedValue({ status: 'performed' });
    render(
      <WindowLifecycleProvider>
        <LifecycleHarness />
      </WindowLifecycleProvider>,
    );
    fireEvent.click(screen.getByRole('button', { name: 'restart' }));
    await screen.findByText('dialog-restartApplication');

    fireEvent.click(screen.getByRole('button', { name: 'cancel-and-continue' }));

    expect(mocks.cancelActiveMutation).toHaveBeenCalledTimes(1);
    expect(mocks.executeLifecycleAction).toHaveBeenCalledTimes(1);

    act(() => {
      useMutationStore.setState({ revision: 2, activeMutation: null });
    });

    await waitFor(() => {
      expect(mocks.executeLifecycleAction).toHaveBeenNthCalledWith(2, 'restartApplication');
    });
  });

  it('does not offer cancellation during a non-cancelable mutation phase', async () => {
    const nonCancelableMutation = { ...mutation, cancelable: false };
    mocks.executeLifecycleAction.mockImplementation(async () => {
      useMutationStore.setState({ revision: 1, activeMutation: nonCancelableMutation });
      return {
        status: 'blocked',
        snapshot: { revision: 1, active: nonCancelableMutation },
      };
    });
    render(
      <WindowLifecycleProvider>
        <LifecycleHarness />
      </WindowLifecycleProvider>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'restart' }));

    await screen.findByText('dialog-restartApplication');
    expect(screen.queryByRole('button', { name: 'cancel-and-continue' })).toBeNull();
  });

  it('coalesces overlapping lifecycle requests in the same window', async () => {
    let resolveAction: (() => void) | undefined;
    mocks.executeLifecycleAction.mockImplementation(() => new Promise((resolve) => {
      resolveAction = () => resolve({ status: 'performed' });
    }));
    render(
      <WindowLifecycleProvider>
        <LifecycleHarness />
      </WindowLifecycleProvider>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'restart' }));
    fireEvent.click(screen.getByRole('button', { name: 'restart' }));

    expect(mocks.executeLifecycleAction).toHaveBeenCalledTimes(1);
    await act(async () => resolveAction?.());
  });

  it('releases native listeners when the window React tree unmounts', async () => {
    const view = render(
      <WindowLifecycleProvider>
        <LifecycleHarness />
      </WindowLifecycleProvider>,
    );
    await waitFor(() => {
      expect(mocks.closeHandler).toBeDefined();
      expect(mocks.lifecycleHandler).toBeDefined();
    });

    view.unmount();

    expect(mocks.closeUnlisten).toHaveBeenCalledTimes(1);
    expect(mocks.lifecycleUnlisten).toHaveBeenCalledTimes(1);
  });
});
