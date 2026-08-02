import { create } from 'zustand';
import { getInstallWizardSession } from '@/hooks/useTauriApi';
import type { InstallWizardSessionSnapshot } from '@/bindings';

interface InstallWizardSessionState {
  revision: number;
  active: boolean;
  loading: boolean;
  syncError: 'monitor' | 'refresh' | null;
  monitorRetryRevision: number;
  snapshotVersion: number;
  acceptSnapshot: (snapshot: InstallWizardSessionSnapshot) => void;
  beginMonitoring: () => void;
  reportMonitorError: (error: unknown) => void;
  retryMonitoring: () => void;
  refreshSession: (options?: { preserveMonitorError?: boolean }) => Promise<void>;
}

export function selectInstallWizardSessionBlocksWrites(
  state: Pick<InstallWizardSessionState, 'active' | 'loading' | 'syncError'>,
): boolean {
  return state.active || state.loading || state.syncError !== null;
}

let refreshPromise: Promise<void> | null = null;
let refreshPreservesMonitorError = false;

export const useInstallWizardSessionStore = create<InstallWizardSessionState>()((set, get) => ({
  revision: 0,
  active: false,
  loading: false,
  syncError: null,
  monitorRetryRevision: 0,
  snapshotVersion: 0,

  acceptSnapshot: (snapshot) => {
    set((state) => ({
      revision: snapshot.revision > state.revision ? snapshot.revision : state.revision,
      active: snapshot.revision > state.revision ? snapshot.active : state.active,
      loading: false,
      syncError: null,
      snapshotVersion: state.snapshotVersion + 1,
    }));
  },

  beginMonitoring: () => set({ loading: true, syncError: null }),

  reportMonitorError: () => set({ loading: false, syncError: 'monitor' }),

  retryMonitoring: () => set((state) => ({
    monitorRetryRevision: state.monitorRetryRevision + 1,
  })),

  refreshSession: (options) => {
    if (options?.preserveMonitorError) refreshPreservesMonitorError = true;
    if (refreshPromise) return refreshPromise;

    const snapshotVersion = get().snapshotVersion;
    set({ loading: true });
    refreshPromise = (async () => {
      try {
        const snapshot = await getInstallWizardSession();
        if (refreshPreservesMonitorError) {
          set((state) => ({
            revision: snapshot.revision > state.revision ? snapshot.revision : state.revision,
            active: snapshot.revision > state.revision ? snapshot.active : state.active,
            loading: false,
            syncError: state.syncError === 'monitor' ? 'monitor' : null,
            snapshotVersion: state.snapshotVersion + 1,
          }));
        } else {
          get().acceptSnapshot(snapshot);
        }
      } catch (error) {
        set((state) => state.snapshotVersion === snapshotVersion
          ? {
              syncError: refreshPreservesMonitorError && state.syncError === 'monitor'
                ? 'monitor'
                : 'refresh',
            }
          : state);
        throw error;
      } finally {
        set({ loading: false });
        refreshPromise = null;
        refreshPreservesMonitorError = false;
      }
    })();
    return refreshPromise;
  },
}));

export function prepareInstallWizardSessionMonitoring(pathname: string): void {
  if (pathname === '/wizard') return;
  useInstallWizardSessionStore.getState().beginMonitoring();
}
