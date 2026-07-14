import { describe, it, expect, vi, beforeEach } from 'vitest';

const mockCheck = vi.fn();
const mockRelaunch = vi.fn();
const mockPlatform = vi.fn();
const mockRefreshMutation = vi.fn();
const mutationState = {
  activeMutation: null as null | { kind: string },
};

vi.mock('@tauri-apps/plugin-updater', () => ({
  check: (...args: unknown[]) => mockCheck(...args),
}));
vi.mock('@tauri-apps/plugin-process', () => ({
  relaunch: (...args: unknown[]) => mockRelaunch(...args),
}));
vi.mock('@tauri-apps/plugin-os', () => ({
  platform: () => mockPlatform(),
}));
vi.mock('@/stores/mutation', () => ({
  useMutationStore: {
    getState: () => ({
      activeMutation: mutationState.activeMutation,
      refreshMutation: mockRefreshMutation,
    }),
  },
}));

import { relaunchApp, useUpdaterStore } from '../updater';

describe('useUpdaterStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mutationState.activeMutation = null;
    mockRefreshMutation.mockResolvedValue(undefined);
    localStorage.clear();
    useUpdaterStore.setState({
      status: 'idle',
      newVersion: null,
      releaseNotes: null,
      downloadProgress: 0,
      error: null,
      currentPlatform: null,
      lastCheckTime: null,
    });
  });

  describe('concurrency guard', () => {
    it('checkForUpdate is no-op when status is checking', async () => {
      useUpdaterStore.setState({ status: 'checking' });
      await useUpdaterStore.getState().checkForUpdate();
      expect(mockCheck).not.toHaveBeenCalled();
    });

    it('checkForUpdate is no-op when status is available', async () => {
      useUpdaterStore.setState({ status: 'available' });
      await useUpdaterStore.getState().checkForUpdate();
      expect(mockCheck).not.toHaveBeenCalled();
    });

    it('checkForUpdate is no-op when status is downloading', async () => {
      useUpdaterStore.setState({ status: 'downloading' });
      await useUpdaterStore.getState().checkForUpdate();
      expect(mockCheck).not.toHaveBeenCalled();
    });

    it('checkForUpdate is no-op when status is ready', async () => {
      useUpdaterStore.setState({ status: 'ready' });
      await useUpdaterStore.getState().checkForUpdate();
      expect(mockCheck).not.toHaveBeenCalled();
    });

    it('checkForUpdate runs when status is error', async () => {
      mockCheck.mockResolvedValue(null);
      mockPlatform.mockReturnValue('windows');
      useUpdaterStore.setState({ status: 'error' });
      await useUpdaterStore.getState().checkForUpdate();
      expect(mockCheck).toHaveBeenCalled();
    });

    it('downloadAndInstall is no-op when status is not available', async () => {
      useUpdaterStore.setState({ status: 'idle' });
      await useUpdaterStore.getState().downloadAndInstall();
      expect(useUpdaterStore.getState().status).toBe('idle');
    });
  });

  describe('checkForUpdate', () => {
    it('sets idle when no update available', async () => {
      mockCheck.mockResolvedValue(null);
      mockPlatform.mockReturnValue('windows');
      await useUpdaterStore.getState().checkForUpdate();
      expect(useUpdaterStore.getState().status).toBe('idle');
      expect(useUpdaterStore.getState().lastCheckTime).not.toBeNull();
    });

    it('sets available with version and releaseNotes when update found', async () => {
      mockCheck.mockResolvedValue({
        version: '1.2.0',
        body: '## Changelog\n- Fix bug',
        downloadAndInstall: vi.fn(),
      });
      mockPlatform.mockReturnValue('windows');
      await useUpdaterStore.getState().checkForUpdate();
      const state = useUpdaterStore.getState();
      expect(state.status).toBe('available');
      expect(state.newVersion).toBe('1.2.0');
      expect(state.releaseNotes).toBe('## Changelog\n- Fix bug');
    });

    it('does NOT auto-download on any platform', async () => {
      const mockDownload = vi.fn();
      mockCheck.mockResolvedValue({
        version: '1.2.0',
        body: '',
        downloadAndInstall: mockDownload,
      });
      mockPlatform.mockReturnValue('windows');
      await useUpdaterStore.getState().checkForUpdate();
      expect(mockDownload).not.toHaveBeenCalled();
      expect(useUpdaterStore.getState().status).toBe('available');
    });

    it('sets error on check failure and records lastCheckTime', async () => {
      mockCheck.mockRejectedValue(new Error('Network error'));
      await useUpdaterStore.getState().checkForUpdate();
      const state = useUpdaterStore.getState();
      expect(state.status).toBe('error');
      expect(state.error).toBe('Network error');
      expect(state.lastCheckTime).not.toBeNull();
    });
  });

  describe('shouldAutoCheck', () => {
    it('returns true when no last check', () => {
      expect(useUpdaterStore.getState().shouldAutoCheck()).toBe(true);
    });

    it('returns false within 24h of successful check', () => {
      const now = Date.now();
      localStorage.setItem('updater_last_check', now.toString());
      useUpdaterStore.setState({ lastCheckTime: now });
      expect(useUpdaterStore.getState().shouldAutoCheck()).toBe(false);
    });

    it('returns true after 24h of successful check', () => {
      const oldTime = Date.now() - 25 * 60 * 60 * 1000;
      localStorage.setItem('updater_last_check', oldTime.toString());
      useUpdaterStore.setState({ lastCheckTime: oldTime });
      expect(useUpdaterStore.getState().shouldAutoCheck()).toBe(true);
    });

    it('returns true after 4h of failed check', () => {
      const oldTime = Date.now() - 5 * 60 * 60 * 1000;
      localStorage.setItem('updater_last_check', oldTime.toString());
      localStorage.setItem('updater_last_check_error', 'true');
      useUpdaterStore.setState({ lastCheckTime: oldTime });
      expect(useUpdaterStore.getState().shouldAutoCheck()).toBe(true);
    });

    it('returns false within 4h of failed check', () => {
      const oldTime = Date.now() - 2 * 60 * 60 * 1000;
      localStorage.setItem('updater_last_check', oldTime.toString());
      localStorage.setItem('updater_last_check_error', 'true');
      useUpdaterStore.setState({ lastCheckTime: oldTime });
      expect(useUpdaterStore.getState().shouldAutoCheck()).toBe(false);
    });
  });

  describe('dismiss', () => {
    it('resets state to idle', () => {
      useUpdaterStore.setState({
        status: 'available',
        newVersion: '1.2.0',
        releaseNotes: 'notes',
        downloadProgress: 50,
      });
      useUpdaterStore.getState().dismiss();
      const state = useUpdaterStore.getState();
      expect(state.status).toBe('idle');
      expect(state.newVersion).toBeNull();
      expect(state.releaseNotes).toBeNull();
      expect(state.downloadProgress).toBe(0);
    });
  });

  describe('relaunchApp', () => {
    it('relaunches after refreshing and confirming there is no active mutation', async () => {
      const result = await relaunchApp();

      expect(mockRefreshMutation).toHaveBeenCalledTimes(1);
      expect(mockRelaunch).toHaveBeenCalledTimes(1);
      expect(result).toBe('relaunched');
    });

    it('blocks relaunch while a mutation is active', async () => {
      mutationState.activeMutation = { kind: 'install' };

      const result = await relaunchApp();

      expect(mockRefreshMutation).toHaveBeenCalledTimes(1);
      expect(mockRelaunch).not.toHaveBeenCalled();
      expect(result).toBe('blocked');
    });
  });
});
