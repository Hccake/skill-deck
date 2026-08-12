/* @vitest-environment jsdom */

import '@/test-utils';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';
import { InstallWizardStatusControl } from '../InstallWizardStatusControl';

const mocks = vi.hoisted(() => ({
  continueInstallFlow: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock('@/workflows/install-session-feedback', () => ({
  continueInstallFlow: (...args: unknown[]) => mocks.continueInstallFlow(...args),
}));

vi.mock('sonner', () => ({
  toast: { error: mocks.toastError },
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

function renderControl() {
  return render(
    <TooltipProvider>
      <InstallWizardStatusControl />
    </TooltipProvider>,
  );
}

describe('InstallWizardStatusControl', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.continueInstallFlow.mockResolvedValue(undefined);
    useInstallWizardSessionStore.setState({
      revision: 0,
      active: false,
      loading: false,
      hasConfirmedSnapshot: true,
      syncError: null,
      monitorRetryRevision: 0,
      snapshotVersion: 0,
    });
  });

  it('does not occupy header space while the confirmed session is inactive', () => {
    renderControl();
    expect(screen.queryByRole('button')).toBeNull();

    act(() => useInstallWizardSessionStore.setState({ loading: true }));
    expect(screen.queryByRole('button')).toBeNull();
  });

  it('continues the open flow from the active-session explanation', async () => {
    useInstallWizardSessionStore.setState({ active: true });
    renderControl();

    const button = screen.getByRole('button', {
      name: 'installWizardSession.writeUnavailable',
    });
    fireEvent.click(button);
    await waitFor(() => expect(mocks.continueInstallFlow).toHaveBeenCalledTimes(1));
  });

  it('prioritizes monitoring recovery over a stale active snapshot', () => {
    useInstallWizardSessionStore.setState({ active: true, syncError: 'monitor' });
    renderControl();

    expect(screen.queryByText('installWizardSession.writeUnavailable')).toBeNull();
    fireEvent.click(screen.getByRole('button', {
      name: 'installWizardSession.syncFailedTitle',
    }));

    expect(useInstallWizardSessionStore.getState().monitorRetryRevision).toBe(1);
    expect(mocks.continueInstallFlow).not.toHaveBeenCalled();
  });

  it('reports a real failure to open the installation window', async () => {
    mocks.continueInstallFlow.mockRejectedValue(new Error('focus failed'));
    useInstallWizardSessionStore.setState({ active: true });
    renderControl();

    fireEvent.click(screen.getByRole('button', {
      name: 'installWizardSession.writeUnavailable',
    }));

    await waitFor(() => expect(mocks.toastError).toHaveBeenCalledWith(
      'installWizardSession.focusFailed',
    ));
  });
});
