import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useMutationStore } from '@/stores/mutation';
import type { MutationSnapshot } from '@/bindings';

export function useMutationMonitor(_pollIntervalMs = 2_000) {
  const refreshMutation = useMutationStore((state) => state.refreshMutation);
  const acceptSnapshot = useMutationStore((state) => state.acceptSnapshot);

  useEffect(() => {
    const refresh = () => {
      void refreshMutation().catch((error) => {
        console.error('Failed to refresh active mutation:', error);
      });
    };

    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<MutationSnapshot>('mutation-state-changed', (event) => {
      acceptSnapshot(event.payload);
    }).then((stopListening) => {
      if (disposed) {
        stopListening();
      } else {
        unlisten = stopListening;
        window.addEventListener('focus', refresh);
        refresh();
      }
    }).catch((error) => {
      console.error('Failed to monitor mutation state:', error);
      if (!disposed) {
        window.addEventListener('focus', refresh);
        refresh();
      }
    });

    return () => {
      disposed = true;
      window.removeEventListener('focus', refresh);
      unlisten?.();
    };
  }, [acceptSnapshot, refreshMutation]);
}
