import { create } from 'zustand';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { platform } from '@tauri-apps/plugin-os';

const LAST_CHECK_KEY = 'updater_last_check';
const CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000; // 24 hours

type UpdateStatus =
  | 'idle'
  | 'checking'
  | 'available'
  | 'downloading'
  | 'ready'
  | 'error';

type LastCheckResult = 'none' | 'up-to-date' | 'available' | 'error';

interface UpdaterState {
  status: UpdateStatus;
  lastCheckResult: LastCheckResult;
  newVersion: string | null;
  downloadProgress: number;
  error: string | null;
  currentPlatform: string | null;

  checkForUpdate: () => Promise<void>;
  downloadAndInstall: () => Promise<void>;
  dismiss: () => void;
  shouldAutoCheck: () => boolean;
}

// 模块级变量持有当前 Update 对象引用（不放进 store 避免序列化问题）
let pendingUpdate: Update | null = null;

export const useUpdaterStore = create<UpdaterState>((set, get) => ({
  status: 'idle',
  lastCheckResult: 'none',
  newVersion: null,
  downloadProgress: 0,
  error: null,
  currentPlatform: null,

  shouldAutoCheck: () => {
    const lastCheck = localStorage.getItem(LAST_CHECK_KEY);
    if (!lastCheck) return true;
    return Date.now() - parseInt(lastCheck, 10) > CHECK_INTERVAL_MS;
  },

  checkForUpdate: async () => {
    set({ status: 'checking', error: null });
    try {
      const update = await check();
      localStorage.setItem(LAST_CHECK_KEY, Date.now().toString());

      if (!update) {
        set({ status: 'idle', newVersion: null, lastCheckResult: 'up-to-date' });
        return;
      }

      pendingUpdate = update;
      const currentPlatform = platform();
      set({
        status: 'available',
        lastCheckResult: 'available',
        newVersion: update.version,
        currentPlatform,
      });

      // Windows/Linux: 自动开始下载
      if (currentPlatform !== 'macos') {
        get().downloadAndInstall();
      }
    } catch (e) {
      console.error('Update check failed:', e);
      set({
        status: 'error',
        lastCheckResult: 'error',
        error: e instanceof Error ? e.message : String(e),
      });
    }
  },

  downloadAndInstall: async () => {
    if (!pendingUpdate) return;

    set({ status: 'downloading', downloadProgress: 0 });
    try {
      let totalBytes = 0;
      let downloadedBytes = 0;

      await pendingUpdate.downloadAndInstall((event) => {
        switch (event.event) {
          case 'Started':
            totalBytes = event.data.contentLength ?? 0;
            break;
          case 'Progress':
            downloadedBytes += event.data.chunkLength;
            if (totalBytes > 0) {
              set({
                downloadProgress: Math.round(
                  (downloadedBytes / totalBytes) * 100
                ),
              });
            }
            break;
          case 'Finished':
            break;
        }
      });

      set({ status: 'ready', downloadProgress: 100 });
    } catch (e) {
      console.error('Download failed:', e);
      set({
        status: 'error',
        error: e instanceof Error ? e.message : String(e),
      });
    }
  },

  dismiss: () => {
    pendingUpdate = null;
    set({ status: 'idle', newVersion: null, downloadProgress: 0, error: null });
  },
}));

/** 用户确认重启 */
export async function relaunchApp() {
  await relaunch();
}
