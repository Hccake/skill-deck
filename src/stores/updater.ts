import { create } from 'zustand';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { platform } from '@tauri-apps/plugin-os';

const LAST_CHECK_KEY = 'updater_last_check';
const LAST_CHECK_ERROR_KEY = 'updater_last_check_error';
const CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000; // 24 hours
const ERROR_RETRY_INTERVAL_MS = 4 * 60 * 60 * 1000; // 4 hours

type UpdateStatus =
  | 'idle'
  | 'checking'
  | 'available'
  | 'downloading'
  | 'ready'
  | 'error';

interface UpdaterState {
  status: UpdateStatus;
  newVersion: string | null;
  releaseNotes: string | null;
  downloadProgress: number;
  error: string | null;
  currentPlatform: string | null;
  lastCheckTime: number | null;

  checkForUpdate: () => Promise<void>;
  downloadAndInstall: () => Promise<void>;
  dismiss: () => void;
  shouldAutoCheck: () => boolean;
}

// 模块级变量 — 不放进 store 避免序列化问题
let pendingUpdate: Update | null = null;
let abortFlag = false;

// js-cache-storage: 启动时从 localStorage 恢复到内存，避免重复 I/O
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
  error: null,
  currentPlatform: null,
  lastCheckTime: readLastCheckTime(),

  shouldAutoCheck: () => {
    // js-cache-storage: 优先用内存中的 lastCheckTime，减少 localStorage 读取
    const { lastCheckTime } = get();
    if (!lastCheckTime) return true;
    try {
      const wasError = localStorage.getItem(LAST_CHECK_ERROR_KEY) === 'true';
      const interval = wasError ? ERROR_RETRY_INTERVAL_MS : CHECK_INTERVAL_MS;
      return Date.now() - lastCheckTime > interval;
    } catch {
      return true;
    }
  },

  checkForUpdate: async () => {
    const { status } = get();
    // 并发保护：仅 idle/error 可触发
    if (status !== 'idle' && status !== 'error') return;

    set({ status: 'checking', error: null });
    try {
      const update = await check();
      const now = Date.now();
      localStorage.setItem(LAST_CHECK_KEY, now.toString());
      localStorage.removeItem(LAST_CHECK_ERROR_KEY);

      if (!update) {
        set({ status: 'idle', newVersion: null, releaseNotes: null, lastCheckTime: now });
        return;
      }

      pendingUpdate = update;
      const currentPlatform = platform();
      set({
        status: 'available',
        newVersion: update.version,
        releaseNotes: update.body ?? null,
        currentPlatform,
        lastCheckTime: now,
      });
      // 不再自动下载 — 等待用户在 Dialog 中确认
    } catch (e) {
      const now = Date.now();
      localStorage.setItem(LAST_CHECK_KEY, now.toString());
      localStorage.setItem(LAST_CHECK_ERROR_KEY, 'true');
      console.error('Update check failed:', e);
      set({
        status: 'error',
        lastCheckTime: now,
        error: e instanceof Error ? e.message : String(e),
      });
    }
  },

  downloadAndInstall: async () => {
    // Guard：仅 available 状态可触发
    if (get().status !== 'available') return;
    if (!pendingUpdate) return;

    abortFlag = false;
    set({ status: 'downloading', downloadProgress: 0 });
    try {
      let totalBytes = 0;
      let downloadedBytes = 0;

      await pendingUpdate.downloadAndInstall((event) => {
        if (abortFlag) return;
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

      // 下载完成后检查是否已被 dismiss
      if (abortFlag) return;
      set({ status: 'ready', downloadProgress: 100 });
    } catch (e) {
      if (abortFlag) return;
      console.error('Download failed:', e);
      set({
        status: 'error',
        error: e instanceof Error ? e.message : String(e),
      });
    }
  },

  dismiss: () => {
    abortFlag = true;
    pendingUpdate = null;
    set({
      status: 'idle',
      newVersion: null,
      releaseNotes: null,
      downloadProgress: 0,
      error: null,
    });
  },
}));
