/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useUpdaterStore } from '@/stores/updater';
import { UpdateDialog } from '../update-dialog';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@/lifecycle/useWindowLifecycle', () => ({
  useWindowLifecycle: () => ({ requestAction: vi.fn() }),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  checkApplicationUpdate: vi.fn(),
  downloadAndInstallApplicationUpdate: vi.fn(),
  cancelApplicationUpdateDownload: vi.fn(),
}));

describe('UpdateDialog', () => {
  beforeEach(() => {
    useInstallWizardSessionStore.setState({ revision: 0, active: false, loading: false });
    useUpdaterStore.setState({
      status: 'error',
      newVersion: '2.0.0',
      releaseNotes: null,
      downloadProgress: 37,
      downloadedBytes: 37,
      totalBytes: 100,
      error: {
        kind: 'executionFailed',
        data: { message: 'private backend diagnostic' },
      },
      failedOperation: 'install',
      lastCheckTime: null,
      dialogVisible: true,
    });
  });

  it('renders an accessible stable error and offers retry without exposing diagnostics', () => {
    const retry = vi.fn().mockResolvedValue(undefined);
    useUpdaterStore.setState({ retry });
    render(<UpdateDialog open />);

    expect(screen.getByRole('alert').textContent).toContain('settings.update.installError');
    expect(screen.queryByText('private backend diagnostic')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'settings.update.retry' }));
    expect(retry).toHaveBeenCalledTimes(1);
  });

  it('announces Backend-owned download progress', () => {
    const cancelDownload = vi.fn().mockResolvedValue(undefined);
    useUpdaterStore.setState({ status: 'downloading', error: null, cancelDownload });
    render(<UpdateDialog open />);

    expect(screen.getByRole('status').getAttribute('aria-live')).toBe('polite');
    expect(screen.getByRole('progressbar').getAttribute('aria-valuenow')).toBe('37');
    fireEvent.click(screen.getByRole('button', { name: 'settings.update.cancel' }));
    expect(cancelDownload).toHaveBeenCalledTimes(1);
  });

  it('blocks update installation and restart while the wizard is open', () => {
    useInstallWizardSessionStore.setState({ revision: 1, active: true });
    useUpdaterStore.setState({ status: 'available', error: null, failedOperation: null });
    const view = render(<UpdateDialog open />);
    expect((screen.getByRole('button', { name: 'settings.update.updateNow' }) as HTMLButtonElement).disabled)
      .toBe(true);

    view.unmount();
    useUpdaterStore.setState({ status: 'ready' });
    render(<UpdateDialog open />);
    expect((screen.getByRole('button', { name: 'settings.update.restartNow' }) as HTMLButtonElement).disabled)
      .toBe(true);
  });
});
