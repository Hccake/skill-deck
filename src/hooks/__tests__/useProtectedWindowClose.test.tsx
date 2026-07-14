/* @vitest-environment jsdom */

import '@/test-utils';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ActiveMutation } from '@/bindings';
import { useMutationStore } from '@/stores/mutation';
import { MutationInterruptionDialog } from '@/components/layout/MutationInterruptionDialog';
import { useProtectedWindowClose } from '../useProtectedWindowClose';

const mutation: ActiveMutation = {
  kind: 'install',
  context: {
    environment: { kind: 'wsl', distro_name: 'Ubuntu' },
    scope: { scope: 'global' },
  },
  id: 'mutation-1',
  phase: 'preparing',
  progress: null,
  cancelable: true,
};

const mocks = vi.hoisted(() => ({
  close: vi.fn().mockResolvedValue(undefined),
  unlisten: vi.fn(),
  onCloseRequested: vi.fn(),
  getActiveMutation: vi.fn(),
  requestCancelActiveMutation: vi.fn(),
  closeHandler: undefined as undefined | ((event: { preventDefault: () => void }) => Promise<void>),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({
    close: mocks.close,
    onCloseRequested: mocks.onCloseRequested,
  }),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  getActiveMutation: (...args: unknown[]) => mocks.getActiveMutation(...args),
  requestCancelActiveMutation: (...args: unknown[]) => mocks.requestCancelActiveMutation(...args),
}));

function CloseHarness() {
  const closeProtection = useProtectedWindowClose();

  return <MutationInterruptionDialog {...closeProtection.dialogProps} />;
}

async function requestWindowClose() {
  const preventDefault = vi.fn();
  await act(async () => {
    await mocks.closeHandler?.({ preventDefault });
  });
  return preventDefault;
}

describe('useProtectedWindowClose', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.closeHandler = undefined;
    mocks.onCloseRequested.mockImplementation(async (handler) => {
      mocks.closeHandler = handler;
      return mocks.unlisten;
    });
    useMutationStore.setState({
      revision: 0,
      activeMutation: null,
      loading: false,
      cancelling: false,
    });
  });

  it('closes normally after confirming there is no active mutation', async () => {
    mocks.getActiveMutation.mockResolvedValue({ revision: 1, active: null });
    render(<CloseHarness />);
    await waitFor(() => expect(mocks.closeHandler).toBeDefined());

    const preventDefault = await requestWindowClose();

    expect(preventDefault).toHaveBeenCalledTimes(1);
    expect(mocks.close).toHaveBeenCalledTimes(1);
    expect(screen.queryByText('mutation.interruption.closeTitle')).toBeNull();
  });

  it('keeps the window open when the user chooses to continue waiting', async () => {
    mocks.getActiveMutation.mockResolvedValue({ revision: 1, active: mutation });
    render(<CloseHarness />);
    await waitFor(() => expect(mocks.closeHandler).toBeDefined());

    await requestWindowClose();

    expect(mocks.close).not.toHaveBeenCalled();
    expect(screen.getByText('mutation.interruption.closeTitle')).toBeDefined();

    fireEvent.click(screen.getByRole('button', {
      name: 'mutation.interruption.continueWaiting',
    }));

    await waitFor(() => {
      expect(screen.queryByText('mutation.interruption.closeTitle')).toBeNull();
    });
    expect(mocks.close).not.toHaveBeenCalled();
  });

  it('cancels the mutation and closes only after polling observes completion', async () => {
    mocks.getActiveMutation
      .mockResolvedValueOnce({ revision: 1, active: mutation })
      .mockResolvedValueOnce({ revision: 2, active: mutation })
      .mockResolvedValue({ revision: 3, active: null });
    mocks.requestCancelActiveMutation.mockResolvedValue(true);
    render(<CloseHarness />);
    await waitFor(() => expect(mocks.closeHandler).toBeDefined());

    await requestWindowClose();
    fireEvent.click(screen.getByRole('button', {
      name: 'mutation.interruption.cancelAndClose',
    }));

    await waitFor(() => expect(mocks.requestCancelActiveMutation).toHaveBeenCalledTimes(1));
    expect(mocks.close).not.toHaveBeenCalled();
    expect(screen.getByText('mutation.interruption.cancelling')).toBeDefined();

    await act(async () => {
      await useMutationStore.getState().refreshMutation();
    });

    await waitFor(() => expect(mocks.close).toHaveBeenCalledTimes(1));
  });

  it('does not offer cancellation when the active mutation is not cancelable', async () => {
    mocks.getActiveMutation.mockResolvedValue({
      revision: 1,
      active: { ...mutation, cancelable: false },
    });
    render(<CloseHarness />);
    await waitFor(() => expect(mocks.closeHandler).toBeDefined());

    await requestWindowClose();

    expect(screen.getByText('mutation.interruption.closeTitle')).toBeDefined();
    expect(screen.queryByRole('button', {
      name: 'mutation.interruption.cancelAndClose',
    })).toBeNull();
    expect(screen.getByRole('button', {
      name: 'mutation.interruption.continueWaiting',
    })).toBeDefined();
  });
});
