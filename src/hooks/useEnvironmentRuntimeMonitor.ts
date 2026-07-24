import { useEffect } from 'react';
import { events } from '@/bindings';
import {
  useEnvironmentStore,
  type EnvironmentDiscoveryIntent,
} from '@/stores/environment';

export function useEnvironmentRuntimeMonitor() {
  const applyRuntimeEvent = useEnvironmentStore((state) => state.applyRuntimeEvent);
  const discover = useEnvironmentStore((state) => state.discover);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let subscribing = false;

    const refreshSnapshot = async (intent: EnvironmentDiscoveryIntent) => {
      if (disposed) return;
      try {
        await discover(intent);
      } catch (error) {
        if (!disposed) console.error('Failed to refresh environment runtime state:', error);
      }
    };

    const subscribe = async (intent: EnvironmentDiscoveryIntent) => {
      if (disposed || subscribing || unlisten) return;
      subscribing = true;
      try {
        const stopListening = await events.environmentRuntimeEvent.listen((event) => {
          if (!disposed) applyRuntimeEvent(event.payload);
        });
        if (disposed) {
          stopListening();
          return;
        }
        unlisten = stopListening;
      } catch (error) {
        if (!disposed) console.error('Failed to monitor environment runtime state:', error);
      } finally {
        subscribing = false;
      }
      await refreshSnapshot(intent);
    };

    void subscribe('initial');
    const refreshOnFocus = () => {
      if (unlisten) void refreshSnapshot('resume');
      else void subscribe('resume');
    };
    window.addEventListener('focus', refreshOnFocus);

    return () => {
      disposed = true;
      unlisten?.();
      window.removeEventListener('focus', refreshOnFocus);
    };
  }, [applyRuntimeEvent, discover]);
}
