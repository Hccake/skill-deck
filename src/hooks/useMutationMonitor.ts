import { useEffect } from 'react';
import { useMutationStore } from '@/stores/mutation';

export function useMutationMonitor(pollIntervalMs = 2_000) {
  const refreshMutation = useMutationStore((state) => state.refreshMutation);

  useEffect(() => {
    const refresh = () => {
      void refreshMutation().catch((error) => {
        console.error('Failed to refresh active mutation:', error);
      });
    };

    refresh();
    window.addEventListener('focus', refresh);
    const interval = window.setInterval(refresh, pollIntervalMs);

    return () => {
      window.removeEventListener('focus', refresh);
      window.clearInterval(interval);
    };
  }, [pollIntervalMs, refreshMutation]);
}
