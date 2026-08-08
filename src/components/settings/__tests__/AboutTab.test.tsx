/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useUpdaterStore } from '@/stores/updater';
import { AboutTab } from '../AboutTab';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@tauri-apps/api/app', () => ({
  getVersion: vi.fn(() => new Promise(() => {})),
}));

vi.mock('@/lifecycle/useWindowLifecycle', () => ({
  useWindowLifecycle: () => ({ requestAction: vi.fn() }),
}));

describe('AboutTab updater actions', () => {
  const checkForUpdate = vi.fn().mockResolvedValue(undefined);
  const showDialog = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    useUpdaterStore.setState({
      status: 'idle',
      newVersion: null,
      releaseNotes: null,
      downloadProgress: 0,
      downloadedBytes: 0,
      totalBytes: null,
      error: null,
      failedOperation: null,
      lastCheckTime: null,
      dialogVisible: false,
      checkForUpdate,
      showDialog,
    });
  });

  it('reopens a hidden install failure so the same version can be retried', () => {
    useUpdaterStore.setState({
      status: 'error',
      newVersion: '2.0.0',
      failedOperation: 'install',
      error: { kind: 'custom', data: { message: 'install failed' } },
    });

    render(<AboutTab />);
    fireEvent.click(screen.getByRole('button', { name: 'settings.update.viewUpdate' }));

    expect(showDialog).toHaveBeenCalledTimes(1);
    expect(checkForUpdate).not.toHaveBeenCalled();
    expect(screen.getByText('settings.update.installError')).toBeTruthy();
  });

  it('keeps a failed update check on the check-again path', () => {
    useUpdaterStore.setState({
      status: 'error',
      failedOperation: 'check',
      error: { kind: 'custom', data: { message: 'check failed' } },
    });

    render(<AboutTab />);
    fireEvent.click(screen.getByRole('button', { name: 'settings.update.checkForUpdates' }));

    expect(checkForUpdate).toHaveBeenCalledTimes(1);
    expect(showDialog).not.toHaveBeenCalled();
  });

  it('shows the third-party skills CLI reference version', () => {
    render(<AboutTab />);

    expect(screen.getByText(/settings\.links\.cliReferenceVersion v1\.5\.13/)).toBeTruthy();
  });
});
