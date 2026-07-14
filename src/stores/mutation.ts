import { create } from 'zustand';
import {
  getActiveMutation,
  requestCancelActiveMutation,
} from '@/hooks/useTauriApi';
import type { ActiveMutation } from '@/bindings';

interface MutationState {
  activeMutation: ActiveMutation | null;
  loading: boolean;
  cancelling: boolean;
  refreshMutation: () => Promise<void>;
  cancelActiveMutation: () => Promise<boolean>;
  isWriteBlocked: () => boolean;
  canBrowse: () => boolean;
}

let refreshPromise: Promise<void> | null = null;

export const useMutationStore = create<MutationState>()((set, get) => ({
  activeMutation: null,
  loading: false,
  cancelling: false,

  refreshMutation: () => {
    if (refreshPromise) return refreshPromise;

    set({ loading: true });
    refreshPromise = (async () => {
      try {
        const activeMutation = await getActiveMutation();
        set({
          activeMutation,
          cancelling: activeMutation ? get().cancelling : false,
        });
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
