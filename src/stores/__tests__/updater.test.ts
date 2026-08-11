import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
  install: vi.fn(),
  cancel: vi.fn(),
  getInstallWizardSession: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  checkApplicationUpdate: () => mocks.check(),
  downloadAndInstallApplicationUpdate: (version: string, progress: (event: unknown) => void) =>
    mocks.install(version, progress),
  cancelApplicationUpdateDownload: () => mocks.cancel(),
  getInstallWizardSession: () => mocks.getInstallWizardSession(),
}));

import { useUpdaterStore } from '../updater';
import { useInstallWizardSessionStore } from '../install-wizard-session';

describe('useUpdaterStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    useUpdaterStore.setState({
      status: 'idle', newVersion: null, releaseNotes: null, downloadProgress: 0,
      downloadedBytes: 0, totalBytes: null, error: null, lastCheckTime: null,
      dialogVisible: false, failedOperation: null,
    });
    useInstallWizardSessionStore.setState({
      revision: 0, active: false, loading: false, hasConfirmedSnapshot: false,
      syncError: null, monitorRetryRevision: 0, snapshotVersion: 0,
    });
    mocks.getInstallWizardSession.mockResolvedValue({ revision: 1, active: true });
  });

  it('checks through the Backend and opens the dialog when an update exists', async () => {
    mocks.check.mockResolvedValue({ version: '2.0.0', body: 'notes' });
    await useUpdaterStore.getState().checkForUpdate();
    expect(useUpdaterStore.getState()).toMatchObject({
      status: 'available', newVersion: '2.0.0', releaseNotes: 'notes', dialogVisible: true,
    });
  });

  it('derives progress from the Backend channel and keeps the installed result ready', async () => {
    useUpdaterStore.setState({ status: 'available', newVersion: '2.0.0', dialogVisible: true });
    mocks.install.mockImplementation(async (_version, progress) => {
      progress({ event: 'started', data: { content_length: 100 } });
      progress({ event: 'progress', data: { chunk_length: 40 } });
      progress({ event: 'progress', data: { chunk_length: 60 } });
      progress({ event: 'finished' });
      return { version: '2.0.0', installed: true };
    });

    await useUpdaterStore.getState().downloadAndInstall();
    expect(mocks.install).toHaveBeenCalledWith('2.0.0', expect.any(Function));
    expect(useUpdaterStore.getState()).toMatchObject({ status: 'ready', downloadProgress: 100 });
  });

  it('hides but does not cancel an active Backend update', () => {
    useUpdaterStore.setState({ status: 'downloading', newVersion: '2.0.0', dialogVisible: true });
    useUpdaterStore.getState().dismiss();
    expect(useUpdaterStore.getState()).toMatchObject({
      status: 'downloading', newVersion: '2.0.0', dialogVisible: false,
    });
    useUpdaterStore.getState().showDialog();
    expect(useUpdaterStore.getState().dialogVisible).toBe(true);
  });

  it('cancels only the active download and restores the available update', async () => {
    useUpdaterStore.setState({ status: 'available', newVersion: '2.0.0', dialogVisible: true });
    let rejectDownload: (reason: unknown) => void = () => undefined;
    mocks.install.mockImplementation((_version, progress) => {
      progress({ event: 'started', data: { content_length: 100 } });
      return new Promise((_resolve, reject) => { rejectDownload = reject; });
    });
    mocks.cancel.mockResolvedValue(true);

    const update = useUpdaterStore.getState().downloadAndInstall();
    await vi.waitFor(() => expect(useUpdaterStore.getState().status).toBe('downloading'));
    await useUpdaterStore.getState().cancelDownload();
    expect(mocks.cancel).toHaveBeenCalledTimes(1);
    expect(useUpdaterStore.getState().status).toBe('cancelling');

    rejectDownload({ kind: 'mutationCancelled' });
    await update;
    expect(useUpdaterStore.getState()).toMatchObject({
      status: 'available', error: null, failedOperation: null,
    });
  });

  it('moves to a non-cancelable installing state after the download finishes', async () => {
    useUpdaterStore.setState({ status: 'available', newVersion: '2.0.0', dialogVisible: true });
    mocks.install.mockImplementation(async (_version, progress) => {
      progress({ event: 'started', data: { content_length: 100 } });
      progress({ event: 'downloaded' });
      progress({ event: 'installing' });
      return { version: '2.0.0', installed: true };
    });

    const states: string[] = [];
    const unsubscribe = useUpdaterStore.subscribe((state) => states.push(state.status));
    await useUpdaterStore.getState().downloadAndInstall();
    unsubscribe();

    expect(states).toContain('installing');
    expect(mocks.cancel).not.toHaveBeenCalled();
  });

  it('keeps a failed install visible after dismiss and retries the same version', async () => {
    useUpdaterStore.setState({ status: 'available', newVersion: '2.0.0', dialogVisible: true });
    mocks.install.mockRejectedValueOnce({
      kind: 'executionFailed',
      data: { message: 'private backend diagnostic' },
    });

    await useUpdaterStore.getState().downloadAndInstall();
    expect(useUpdaterStore.getState()).toMatchObject({
      status: 'error',
      newVersion: '2.0.0',
      dialogVisible: true,
    });

    useUpdaterStore.getState().dismiss();
    expect(useUpdaterStore.getState()).toMatchObject({
      status: 'error',
      newVersion: '2.0.0',
      dialogVisible: false,
    });

    mocks.install.mockResolvedValueOnce({ version: '2.0.0', installed: true });
    useUpdaterStore.getState().showDialog();
    await useUpdaterStore.getState().retry();
    expect(mocks.install).toHaveBeenCalledTimes(2);
    expect(useUpdaterStore.getState()).toMatchObject({ status: 'ready', dialogVisible: true });
  });

  it('keeps the existing auto-check retry intervals', () => {
    expect(useUpdaterStore.getState().shouldAutoCheck()).toBe(true);
    const now = Date.now();
    useUpdaterStore.setState({ lastCheckTime: now });
    expect(useUpdaterStore.getState().shouldAutoCheck()).toBe(false);
  });

  it('retries a failed check even when an older install version is still retained', async () => {
    useUpdaterStore.setState({
      status: 'error',
      newVersion: '2.0.0',
      failedOperation: 'check',
      dialogVisible: true,
    });
    mocks.check.mockResolvedValue({ version: '2.1.0', body: 'new notes' });

    await useUpdaterStore.getState().retry();

    expect(mocks.check).toHaveBeenCalledTimes(1);
    expect(mocks.install).not.toHaveBeenCalled();
    expect(useUpdaterStore.getState()).toMatchObject({
      status: 'available',
      newVersion: '2.1.0',
      failedOperation: null,
    });
  });

  it('does not start application installation while the wizard session is active', async () => {
    useInstallWizardSessionStore.setState({ revision: 1, active: true });
    useUpdaterStore.setState({ status: 'available', newVersion: '2.0.0' });

    await useUpdaterStore.getState().downloadAndInstall();

    expect(mocks.install).not.toHaveBeenCalled();
    expect(useUpdaterStore.getState().status).toBe('available');
  });

  it('restores the available state when installation wins application-update admission', async () => {
    useUpdaterStore.setState({ status: 'available', newVersion: '2.0.0', dialogVisible: true });
    mocks.install.mockRejectedValue({ kind: 'installWizardActive' });

    await useUpdaterStore.getState().downloadAndInstall();

    expect(useUpdaterStore.getState()).toMatchObject({
      status: 'available', error: null, dialogVisible: true, failedOperation: null,
    });
  });
});
