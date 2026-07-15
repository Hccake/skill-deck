/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { UpdateDialog } from '../update-dialog';
import { AboutTab } from '../settings/AboutTab';

const mocks = vi.hoisted(() => ({
  requestAction: vi.fn().mockResolvedValue(undefined),
  updaterState: {
    status: 'ready',
    newVersion: '2.0.0',
    releaseNotes: null,
    downloadProgress: 100,
    currentPlatform: 'windows',
    lastCheckTime: null,
    downloadAndInstall: vi.fn(),
    dismiss: vi.fn(),
    checkForUpdate: vi.fn(),
  },
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@tauri-apps/api/app', () => ({
  getVersion: vi.fn().mockResolvedValue('1.0.0'),
}));

vi.mock('@/stores/updater', () => ({
  useUpdaterStore: () => mocks.updaterState,
}));

vi.mock('@/lifecycle/useWindowLifecycle', () => ({
  useWindowLifecycle: () => ({ requestAction: mocks.requestAction }),
}));

describe('relaunch protection entry points', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('routes the update dialog restart action through mutation protection', async () => {
    render(<UpdateDialog open />);

    fireEvent.click(screen.getByRole('button', { name: 'settings.update.restartNow' }));

    await waitFor(() => expect(mocks.requestAction).toHaveBeenCalledTimes(1));
    expect(mocks.requestAction).toHaveBeenCalledWith('restartApplication');
  });

  it('routes the About tab restart action through mutation protection', async () => {
    render(<AboutTab />);

    fireEvent.click(screen.getByRole('button', { name: 'settings.update.restartNow' }));

    await waitFor(() => expect(mocks.requestAction).toHaveBeenCalledTimes(1));
    expect(mocks.requestAction).toHaveBeenCalledWith('restartApplication');
  });
});
