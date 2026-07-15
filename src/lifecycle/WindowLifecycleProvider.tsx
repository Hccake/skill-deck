import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { events } from '@/bindings';
import type { LifecycleAction } from '@/bindings';
import { MutationInterruptionDialog } from '@/components/layout/MutationInterruptionDialog';
import { formatMutationStatus } from '@/lib/mutationStatus';
import { useMutationStore } from '@/stores/mutation';
import { executeLifecycleAction } from './lifecycleApi';
import { WindowLifecycleContext } from './useWindowLifecycle';

function defaultCloseAction(windowLabel: string): LifecycleAction {
  return windowLabel === 'main' ? 'quitApplication' : 'closeCurrentWindow';
}

export function WindowLifecycleProvider({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const appWindow = useMemo(() => getCurrentWebviewWindow(), []);
  const activeMutation = useMutationStore((state) => state.activeMutation);
  const cancelling = useMutationStore((state) => state.cancelling);
  const cancelActiveMutation = useMutationStore((state) => state.cancelActiveMutation);
  const [open, setOpen] = useState(false);
  const [currentAction, setCurrentAction] = useState<LifecycleAction>(() => (
    defaultCloseAction(appWindow.label)
  ));
  const pendingActionRef = useRef<LifecycleAction | null>(null);
  const actionRunningRef = useRef(false);

  const requestAction = useCallback(async (action: LifecycleAction) => {
    if (actionRunningRef.current) return;

    actionRunningRef.current = true;
    setCurrentAction(action);
    try {
      const outcome = await executeLifecycleAction(action);
      if (outcome.status === 'blocked') {
        pendingActionRef.current = action;
        setOpen(true);
      } else {
        pendingActionRef.current = null;
        setOpen(false);
      }
    } catch (error) {
      pendingActionRef.current = null;
      console.error(`Failed to execute lifecycle action ${action}:`, error);
      toast.error(t('mutation.interruption.checkFailed'));
    } finally {
      actionRunningRef.current = false;
    }
  }, [t]);

  useEffect(() => {
    const pendingAction = pendingActionRef.current;
    if (activeMutation || !pendingAction || actionRunningRef.current) return;
    void requestAction(pendingAction);
  }, [activeMutation, requestAction]);

  const continueWaiting = useCallback(() => {
    if (cancelling) return;
    pendingActionRef.current = null;
    setOpen(false);
  }, [cancelling]);

  const cancelAndContinue = useCallback(async () => {
    const pendingAction = pendingActionRef.current;
    if (!pendingAction) return;
    try {
      const accepted = await cancelActiveMutation();
      if (!accepted) await requestAction(pendingAction);
    } catch (error) {
      pendingActionRef.current = null;
      console.error('Failed to cancel active mutation:', error);
      toast.error(t('mutation.interruption.cancelFailed'));
    }
  }, [cancelActiveMutation, requestAction, t]);

  useEffect(() => {
    let disposed = false;
    let stopCloseListener: (() => void) | undefined;
    let stopLifecycleListener: (() => void) | undefined;

    void appWindow.onCloseRequested(async (event) => {
      event.preventDefault();
      await requestAction(defaultCloseAction(appWindow.label));
    }).then((stopListening) => {
      if (disposed) stopListening();
      else stopCloseListener = stopListening;
    }).catch((error) => {
      console.error('Failed to register window close protection:', error);
    });

    void events.lifecycleActionRequestedEvent.listen((event) => {
      if (!disposed) void requestAction(event.payload.action);
    }).then((stopListening) => {
      if (disposed) stopListening();
      else stopLifecycleListener = stopListening;
    }).catch((error) => {
      console.error('Failed to monitor delegated lifecycle actions:', error);
    });

    return () => {
      disposed = true;
      stopCloseListener?.();
      stopLifecycleListener?.();
    };
  }, [appWindow, requestAction]);

  const value = useMemo(() => ({ requestAction }), [requestAction]);

  return (
    <WindowLifecycleContext.Provider value={value}>
      {children}
      <MutationInterruptionDialog
        open={open}
        action={currentAction}
        statusText={activeMutation ? formatMutationStatus(activeMutation, t) : undefined}
        cancelable={activeMutation?.cancelable ?? false}
        cancelling={cancelling}
        onContinueWaiting={continueWaiting}
        onCancelAndContinue={() => void cancelAndContinue()}
      />
    </WindowLifecycleContext.Provider>
  );
}
