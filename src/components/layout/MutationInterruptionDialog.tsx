import { LoaderCircle } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import {
  AlertDialog,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';
import type { MutationInterruptionDialogProps } from '@/hooks/useMutationInterruption';

export function MutationInterruptionDialog({
  open,
  action,
  statusText,
  cancelable,
  cancelling,
  onContinueWaiting,
  onCancelAndContinue,
}: MutationInterruptionDialogProps) {
  const { t } = useTranslation();
  const actionName = action === 'close' ? 'close' : 'restart';
  const descriptionKey = cancelable
    ? `mutation.interruption.${actionName}Description`
    : `mutation.interruption.${actionName}WaitDescription`;

  return (
    <AlertDialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen && !cancelling) onContinueWaiting();
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            {t(`mutation.interruption.${actionName}Title`)}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {t(descriptionKey, {
              status: statusText ?? t('mutation.interruption.unknownStatus'),
            })}
          </AlertDialogDescription>
        </AlertDialogHeader>

        <AlertDialogFooter>
          <Button
            type="button"
            variant="outline"
            disabled={cancelling}
            onClick={onContinueWaiting}
          >
            {t('mutation.interruption.continueWaiting')}
          </Button>
          {cancelling ? (
            <Button type="button" disabled>
              <span className="inline-flex animate-spin" aria-hidden="true">
                <LoaderCircle className="h-4 w-4" />
              </span>
              {t('mutation.interruption.cancelling')}
            </Button>
          ) : cancelable ? (
            <Button type="button" variant="destructive" onClick={onCancelAndContinue}>
              {t(`mutation.interruption.cancelAnd${action === 'close' ? 'Close' : 'Restart'}`)}
            </Button>
          ) : null}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
