/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useUpdaterStore } from '@/stores/updater';
import { AboutTab } from '../AboutTab';

const mocks = vi.hoisted(() => ({
  openDiagnosticsDirectory: vi.fn().mockResolvedValue(undefined),
  readRecentDiagnostics: vi.fn().mockResolvedValue('diagnostic-record'),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@tauri-apps/api/app', () => ({
  getVersion: vi.fn(() => new Promise(() => {})),
}));

vi.mock('@/lifecycle/useWindowLifecycle', () => ({
  useWindowLifecycle: () => ({ requestAction: vi.fn() }),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  openDiagnosticsDirectory: mocks.openDiagnosticsDirectory,
  readRecentDiagnostics: mocks.readRecentDiagnostics,
}));

describe('AboutTab updater actions', () => {
  const checkForUpdate = vi.fn().mockResolvedValue(undefined);
  const showDialog = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: vi.fn().mockResolvedValue(undefined) },
    });
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

  it('shows the vendored skills CLI compatibility baseline', () => {
    render(<AboutTab />);

    expect(screen.getByText(/settings\.links\.cliCompatibility v1\.5\.13/)).toBeTruthy();
  });

  it('opens the local diagnostics directory from the low-frequency support area', async () => {
    render(<AboutTab />);

    fireEvent.click(screen.getByRole('button', { name: 'settings.diagnostics.openDirectory' }));

    await waitFor(() => expect(mocks.openDiagnosticsDirectory).toHaveBeenCalledTimes(1));
  });

  it('copies bounded local diagnostics when the directory cannot be opened', async () => {
    mocks.openDiagnosticsDirectory.mockRejectedValueOnce(new Error('unsupported'));
    render(<AboutTab />);

    fireEvent.click(screen.getByRole('button', { name: 'settings.diagnostics.openDirectory' }));
    await waitFor(() => expect(mocks.openDiagnosticsDirectory).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole('button', { name: 'settings.diagnostics.copy' }));

    await waitFor(() => expect(mocks.readRecentDiagnostics).toHaveBeenCalledTimes(1));
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('diagnostic-record');
  });
});
