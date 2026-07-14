import { create } from 'zustand';
import {
  getActiveMutation,
  requestCancelActiveMutation,
} from '@/hooks/useTauriApi';
import type { ActiveMutation, MutationSnapshot } from '@/bindings';

interface MutationState {
  revision: number;
  activeMutation: ActiveMutation | null;
  loading: boolean;
  cancelling: boolean;
  refreshMutation: () => Promise<void>;
  acceptSnapshot: (snapshot: MutationSnapshot) => void;
  cancelActiveMutation: () => Promise<boolean>;
  isWriteBlocked: () => boolean;
  canBrowse: () => boolean;
}

let refreshPromise: Promise<void> | null = null;

export const useMutationStore = create<MutationState>()((set, get) => ({
  revision: 0,
  activeMutation: null,
  loading: false,
  cancelling: false,

  acceptSnapshot: (snapshot) => {
    set((state) => {
      if (snapshot.revision <= state.revision) return state;
      return {
        revision: snapshot.revision,
        activeMutation: snapshot.active,
        cancelling: snapshot.active ? state.cancelling : false,
      };
    });
  },

  refreshMutation: () => {
    if (refreshPromise) return refreshPromise;

    set({ loading: true });
    refreshPromise = (async () => {
      try {
        get().acceptSnapshot(await getActiveMutation());
      } finally {
        set({ loading: false });
        refreshPromise = null;
      }
    })();
    return refreshPromise;
  },

  cancelActiveMutation: async () => {
    if (get().cancelling) return false;

    set({ cancelling: true });
    try {
      const cancelled = await requestCancelActiveMutation();
      if (cancelled) {
        await get().refreshMutation();
      } else {
        set({ cancelling: false });
      }
      return cancelled;
    } catch (error) {
      set({ cancelling: false });
      throw error;
    }
  },

  isWriteBlocked: () => get().activeMutation !== null,
  canBrowse: () => true,
}));

export function isMutationWriteBlocked(): boolean {
  return useMutationStore.getState().activeMutation !== null;
}
