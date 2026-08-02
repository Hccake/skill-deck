import { create } from 'zustand';
import {
  checkApplicationUpdate,
  downloadAndInstallApplicationUpdate,
} from '@/hooks/useTauriApi';
import { toAppError } from '@/utils/to-app-error';
import type { AppError, ApplicationUpdateProgress } from '@/bindings';
import { isBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';

const LAST_CHECK_KEY = 'updater_last_check';
const LAST_CHECK_ERROR_KEY = 'updater_last_check_error';
const CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;
const ERROR_RETRY_INTERVAL_MS = 4 * 60 * 60 * 1000;

type UpdateStatus = 'idle' | 'checking' | 'available' | 'downloading' | 'ready' | 'error';
type FailedOperation = 'check' | 'install';

interface UpdaterState {
  status: UpdateStatus;
  newVersion: string | null;
  releaseNotes: string | null;
  downloadProgress: number;
  downloadedBytes: number;
  totalBytes: number | null;
  error: AppError | null;
  lastCheckTime: number | null;
  dialogVisible: boolean;
  failedOperation: FailedOperation | null;
  checkForUpdate: () => Promise<void>;
  downloadAndInstall: () => Promise<void>;
  retry: () => Promise<void>;
  dismiss: () => void;
  showDialog: () => void;
  shouldAutoCheck: () => boolean;
}

function readLastCheckTime(): number | null {
  try {
    const stored = localStorage.getItem(LAST_CHECK_KEY);
    return stored ? parseInt(stored, 10) : null;
  } catch {
    return null;
  }
}

export const useUpdaterStore = create<UpdaterState>((set, get) => ({
  status: 'idle',
  newVersion: null,
  releaseNotes: null,
  downloadProgress: 0,
  downloadedBytes: 0,
  totalBytes: null,
  error: null,
  lastCheckTime: readLastCheckTime(),
  dialogVisible: false,
  failedOperation: null,

  shouldAutoCheck: () => {
    const { lastCheckTime } = get();
    if (!lastCheckTime) return true;
    try {
      const interval = localStorage.getItem(LAST_CHECK_ERROR_KEY) === 'true'
        ? ERROR_RETRY_INTERVAL_MS : CHECK_INTERVAL_MS;
      return Date.now() - lastCheckTime > interval;
    } catch {
      return true;
    }
  },

  checkForUpdate: async () => {
    if (!['idle', 'error'].includes(get().status)) {
      if (['available', 'downloading', 'ready'].includes(get().status)) set({ dialogVisible: true });
      return;
    }
    set({ status: 'checking', error: null, failedOperation: null });
    try {
      const update = await checkApplicationUpdate();
      const now = Date.now();
      localStorage.setItem(LAST_CHECK_KEY, String(now));
      localStorage.removeItem(LAST_CHECK_ERROR_KEY);
      if (!update) {
        set({
          status: 'idle', newVersion: null, releaseNotes: null, lastCheckTime: now,
          dialogVisible: false, failedOperation: null,
        });
        return;
      }
      set({
        status: 'available', newVersion: update.version, releaseNotes: update.body,
        lastCheckTime: now, dialogVisible: true, failedOperation: null,
      });
    } catch (error) {
      const now = Date.now();
      localStorage.setItem(LAST_CHECK_KEY, String(now));
      localStorage.setItem(LAST_CHECK_ERROR_KEY, 'true');
      set({
        status: 'error',
        lastCheckTime: now,
        error: toAppError(error),
        dialogVisible: get().dialogVisible,
        failedOperation: 'check',
      });
    }
  },

  downloadAndInstall: async () => {
    if (isBusinessWriteBlocked()) return;
    const version = get().newVersion;
    if (get().status !== 'available' || !version) return;
    set({
      status: 'downloading', downloadProgress: 0, downloadedBytes: 0, totalBytes: null,
      error: null, failedOperation: null,
    });
    const onProgress = (event: ApplicationUpdateProgress) => {
      if (event.event === 'started') {
        set({ totalBytes: event.data.content_length });
      } else if (event.event === 'progress') {
        set((state) => {
          const downloadedBytes = state.downloadedBytes + event.data.chunk_length;
          return {
            downloadedBytes,
            downloadProgress: state.totalBytes && state.totalBytes > 0
              ? Math.min(100, Math.round(downloadedBytes / state.totalBytes * 100)) : state.downloadProgress,
          };
        });
      }
    };
    try {
      const result = await downloadAndInstallApplicationUpdate(version, onProgress);
      set({
        status: result.installed ? 'ready' : 'error',
        downloadProgress: result.installed ? 100 : get().downloadProgress,
        error: result.installed ? null : {
          kind: 'custom',
          data: { message: 'application update did not install' },
        },
        failedOperation: result.installed ? null : 'install',
      });
    } catch (error) {
      set({
        status: 'error', error: toAppError(error), dialogVisible: true,
        failedOperation: 'install',
      });
    }
  },

  retry: async () => {
    const { status, newVersion, failedOperation } = get();
    if (status !== 'error') return;
    if (failedOperation === 'install' && newVersion) {
      set({ status: 'available', error: null, dialogVisible: true });
      await get().downloadAndInstall();
      return;
    }
    await get().checkForUpdate();
  },

  dismiss: () => set({ dialogVisible: false }),
  showDialog: () => set({ dialogVisible: true }),
}));
