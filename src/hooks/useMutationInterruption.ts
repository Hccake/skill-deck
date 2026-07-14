import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { useMutationStore } from '@/stores/mutation';
import { formatMutationStatus } from '@/lib/mutationStatus';

export type MutationInterruptionAction = 'close' | 'restart';
export type ProtectedActionResult = 'performed' | 'relaunched' | 'blocked';

export interface MutationInterruptionDialogProps {
  open: boolean;
  action: MutationInterruptionAction;
  statusText?: string;
  cancelable: boolean;
  cancelling: boolean;
  onContinueWaiting: () => void;
  onCancelAndContinue: () => void;
}

export function useMutationInterruption(
  action: MutationInterruptionAction,
  attemptAction: () => Promise<ProtectedActionResult>,
) {
  const { t } = useTranslation();
  const activeMutation = useMutationStore((state) => state.activeMutation);
  const cancelling = useMutationStore((state) => state.cancelling);
  const cancelActiveMutation = useMutationStore((state) => state.cancelActiveMutation);
  const [open, setOpen] = useState(false);
  const pendingActionRef = useRef(false);
  const actionRunningRef = useRef(false);

  const requestAction = useCallback(async () => {
    if (actionRunningRef.current) return 'blocked' as const;

    actionRunningRef.current = true;
    try {
      const result = await attemptAction();
      if (result === 'blocked') {
        pendingActionRef.current = true;
        setOpen(true);
      } else {
        pendingActionRef.current = false;
        setOpen(false);
      }
      return result;
    } catch (error) {
      pendingActionRef.current = false;
      console.error(`Failed to ${action} application:`, error);
      toast.error(t('mutation.interruption.checkFailed'));
      return 'blocked' as const;
    } finally {
      actionRunningRef.current = false;
    }
  }, [action, attemptAction, t]);

  useEffect(() => {
    if (activeMutation || !pendingActionRef.current || actionRunningRef.current) return;
    void requestAction();
  }, [activeMutation, requestAction]);

  const continueWaiting = useCallback(() => {
    if (cancelling) return;
    pendingActionRef.current = false;
    setOpen(false);
  }, [cancelling]);

  const cancelAndContinue = useCallback(async () => {
    pendingActionRef.current = true;
    try {
      const accepted = await cancelActiveMutation();
      if (!accepted) {
        await requestAction();
      }
    } catch (error) {
      pendingActionRef.current = false;
      console.error('Failed to cancel active mutation:', error);
      toast.error(t('mutation.interruption.cancelFailed'));
    }
  }, [cancelActiveMutation, requestAction, t]);

  return {
    requestAction,
    dialogProps: {
      open,
      action,
      statusText: activeMutation ? formatMutationStatus(activeMutation, t) : undefined,
      cancelable: activeMutation?.cancelable ?? false,
      cancelling,
      onContinueWaiting: continueWaiting,
      onCancelAndContinue: () => void cancelAndContinue(),
    } satisfies MutationInterruptionDialogProps,
  };
}
