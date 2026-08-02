/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';
import { InstallWizardReadOnlyBar } from '../InstallWizardReadOnlyBar';

const mocks = vi.hoisted(() => ({
  focusInstallWizard: vi.fn(),
  getInstallWizardSession: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/hooks/useTauriApi')>(),
  focusInstallWizard: (...args: unknown[]) => mocks.focusInstallWizard(...args),
  getInstallWizardSession: (...args: unknown[]) => mocks.getInstallWizardSession(...args),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe('InstallWizardReadOnlyBar', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.focusInstallWizard.mockResolvedValue(true);
    mocks.getInstallWizardSession.mockResolvedValue({ revision: 2, active: false });
    useInstallWizardSessionStore.setState({
      revision: 0,
      active: false,
      loading: false,
      syncError: null,
      monitorRetryRevision: 0,
      snapshotVersion: 0,
    });
  });

  it('stays hidden without an active install wizard', () => {
    render(<InstallWizardReadOnlyBar />);
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('explains read-only browsing and returns focus to the wizard', async () => {
    useInstallWizardSessionStore.setState({ revision: 1, active: true });
    render(<InstallWizardReadOnlyBar />);

    expect(screen.getByText('installWizardSession.readOnlyDescription')).toBeDefined();
    fireEvent.click(screen.getByRole('button', {
      name: 'installWizardSession.returnToWizard',
    }));

    await waitFor(() => expect(mocks.focusInstallWizard).toHaveBeenCalledTimes(1));
  });

  it('keeps the main window read-only while the initial session check is pending', () => {
    useInstallWizardSessionStore.setState({ loading: true });
    render(<InstallWizardReadOnlyBar />);

    expect(screen.getByText('installWizardSession.checkingDescription')).toBeDefined();
    expect(screen.queryByRole('button')).toBeNull();
  });

  it('shows a retry action when live session monitoring cannot start', () => {
    useInstallWizardSessionStore.setState({ syncError: 'monitor' });
    render(<InstallWizardReadOnlyBar />);

    expect(screen.getByText('installWizardSession.syncFailedDescription')).toBeDefined();
    fireEvent.click(screen.getByRole('button', {
      name: 'installWizardSession.retryMonitoring',
    }));

    expect(useInstallWizardSessionStore.getState().monitorRetryRevision).toBe(1);
  });

  it('keeps the return action when a known active session cannot be refreshed', () => {
    useInstallWizardSessionStore.setState({
      revision: 1,
      active: true,
      syncError: 'refresh',
    });
    render(<InstallWizardReadOnlyBar />);

    expect(screen.getByRole('button', {
      name: 'installWizardSession.returnToWizard',
    })).toBeDefined();
    expect(screen.getByRole('button', {
      name: 'installWizardSession.retryMonitoring',
    })).toBeDefined();
  });

  it('queries the current session again and clears a refresh error after retry', async () => {
    useInstallWizardSessionStore.setState({
      revision: 1,
      active: true,
      syncError: 'refresh',
    });
    render(<InstallWizardReadOnlyBar />);

    fireEvent.click(screen.getByRole('button', {
      name: 'installWizardSession.retryMonitoring',
    }));

    await waitFor(() => expect(mocks.getInstallWizardSession).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(useInstallWizardSessionStore.getState()).toMatchObject({
      revision: 2,
      active: false,
      loading: false,
      syncError: null,
    }));
  });
});
