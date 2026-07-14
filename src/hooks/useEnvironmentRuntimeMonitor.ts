import { useEffect } from 'react';
import { events } from '@/bindings';
import { useEnvironmentStore } from '@/stores/environment';

export function useEnvironmentRuntimeMonitor() {
  const applyRuntimeEvent = useEnvironmentStore((state) => state.applyRuntimeEvent);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void events.environmentRuntimeEvent.listen((event) => {
      if (!disposed) applyRuntimeEvent(event.payload);
    }).then((stopListening) => {
      if (disposed) {
        stopListening();
      } else {
        unlisten = stopListening;
      }
    }).catch((error) => {
      console.error('Failed to monitor environment runtime state:', error);
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [applyRuntimeEvent]);
}
