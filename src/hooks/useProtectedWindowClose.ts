import { useCallback, useEffect, useMemo, useRef } from 'react';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { useMutationStore } from '@/stores/mutation';
import { useMutationInterruption } from './useMutationInterruption';

export function useProtectedWindowClose() {
  const appWindow = useMemo(() => getCurrentWebviewWindow(), []);
  const bypassCloseProtectionRef = useRef(false);

  const attemptClose = useCallback(async () => {
    await useMutationStore.getState().refreshMutation();
    if (useMutationStore.getState().activeMutation) return 'blocked' as const;

    bypassCloseProtectionRef.current = true;
    try {
      await appWindow.close();
      return 'performed' as const;
    } catch (error) {
      bypassCloseProtectionRef.current = false;
      throw error;
    }
  }, [appWindow]);

  const { requestAction, dialogProps } = useMutationInterruption('close', attemptClose);

  useEffect(() => {
    const unlisten = appWindow.onCloseRequested(async (event) => {
      if (bypassCloseProtectionRef.current) return;

      event.preventDefault();
      await requestAction();
    });

    return () => {
      void unlisten.then((stopListening) => stopListening());
    };
  }, [appWindow, requestAction]);

  return {
    requestClose: requestAction,
    dialogProps,
  };
}
