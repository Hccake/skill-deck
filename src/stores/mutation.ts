import { create } from 'zustand';
import {
  getActiveMutation,
  requestCancelActiveMutation,
} from '@/hooks/useTauriApi';
import type { ActiveMutation } from '@/bindings';

interface MutationState {
  activeMutation: ActiveMutation | null;
  loading: boolean;
  refreshMutation: () => Promise<void>;
  cancelActiveMutation: () => Promise<boolean>;
  isWriteBlocked: () => boolean;
  canBrowse: () => boolean;
}

export const useMutationStore = create<MutationState>()((set, get) => ({
  activeMutation: null,
  loading: false,

  refreshMutation: async () => {
    set({ loading: true });
    try {
      set({ activeMutation: await getActiveMutation() });
    } finally {
      set({ loading: false });
    }
  },

  cancelActiveMutation: async () => {
    const cancelled = await requestCancelActiveMutation();
    if (cancelled) await get().refreshMutation();
    return cancelled;
  },

  isWriteBlocked: () => get().activeMutation !== null,
  canBrowse: () => true,
}));
