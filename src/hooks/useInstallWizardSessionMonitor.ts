import { useEffect } from 'react';
import { events } from '@/bindings';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';

export function useInstallWizardSessionMonitor() {
  const acceptSnapshot = useInstallWizardSessionStore((state) => state.acceptSnapshot);
  const beginMonitoring = useInstallWizardSessionStore((state) => state.beginMonitoring);
  const monitorRetryRevision = useInstallWizardSessionStore(
    (state) => state.monitorRetryRevision,
  );
  const reportMonitorError = useInstallWizardSessionStore(
    (state) => state.reportMonitorError,
  );
  const retryMonitoring = useInstallWizardSessionStore((state) => state.retryMonitoring);
  const refreshSession = useInstallWizardSessionStore((state) => state.refreshSession);

  useEffect(() => {
    if (useInstallWizardSessionStore.getState().syncError !== 'monitor') {
      beginMonitoring();
    }

    const refresh = () => {
      void refreshSession().catch((error) => {
        console.error('Failed to refresh install wizard session:', error);
      });
    };

    const refreshAfterMonitorFailure = () => {
      void refreshSession({ preserveMonitorError: true }).catch((error) => {
        console.error('Failed to recover install wizard session snapshot:', error);
      });
    };

    const refreshAfterConnection = () => {
      void refreshSession().catch((error) => {
        console.error('Failed to refresh reconnected install wizard session:', error);
      }).then(() => {
        if (useInstallWizardSessionStore.getState().syncError !== 'monitor') return;
        return refreshSession().catch((error) => {
          console.error('Failed to confirm reconnected install wizard session:', error);
        });
      });
    };

    let disposed = false;
    let unlisten: (() => void) | undefined;
    let listenerState: 'pending' | 'connected' | 'failed' = 'pending';
    let retryRequested = false;
    const recoverOnFocus = () => {
      if (listenerState === 'failed' && !retryRequested) {
        retryRequested = true;
        retryMonitoring();
        return;
      }
      if (listenerState !== 'connected') return;
      const { active, syncError } = useInstallWizardSessionStore.getState();
      if (active || syncError === 'refresh') {
        refresh();
      }
    };
    window.addEventListener('focus', recoverOnFocus);
    void events.installWizardSessionSnapshot.listen((event) => {
      if (!disposed) acceptSnapshot(event.payload);
    }).then((stopListening) => {
      if (disposed) {
        stopListening();
      } else {
        listenerState = 'connected';
        unlisten = stopListening;
        refreshAfterConnection();
      }
    }).catch((error) => {
      console.error('Failed to monitor install wizard session:', error);
      if (!disposed) {
        listenerState = 'failed';
        reportMonitorError(error);
        refreshAfterMonitorFailure();
      }
    });

    return () => {
      disposed = true;
      window.removeEventListener('focus', recoverOnFocus);
      unlisten?.();
    };
  }, [
    acceptSnapshot,
    beginMonitoring,
    monitorRetryRevision,
    refreshSession,
    reportMonitorError,
    retryMonitoring,
  ]);
}
