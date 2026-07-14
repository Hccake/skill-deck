import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import {
  confirmProjectRemoval,
  type ProjectRemovalRequest,
} from '@/stores/project-removal';
import { useMutationStore } from '@/stores/mutation';

interface RemoveProjectDialogProps {
  request: ProjectRemovalRequest | null;
  onClose: () => void;
  onRemoved?: (request: ProjectRemovalRequest) => void;
}

export function RemoveProjectDialog({ request, onClose, onRemoved }: RemoveProjectDialogProps) {
  const { t } = useTranslation();
  const writeBlocked = useMutationStore((state) => state.activeMutation !== null);
  const [submitting, setSubmitting] = useState(false);

  const confirm = async () => {
    if (!request || writeBlocked || submitting) return;
    const completedRequest = request;
    setSubmitting(true);
    try {
      await confirmProjectRemoval(completedRequest);
      onClose();
      onRemoved?.(completedRequest);
    } catch (error) {
      console.error('Failed to remove project:', error);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <AlertDialog
      open={request !== null}
      onOpenChange={(open) => {
        if (!open && !submitting) onClose();
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('context.removeConfirm.title')}</AlertDialogTitle>
          <AlertDialogDescription className="space-y-2">
            <span className="block">
              {t('context.removeConfirm.description', { name: request?.projectName ?? '' })}
            </span>
            <span className="block">{t('context.removeConfirm.unregisterOnly')}</span>
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={submitting}>
            {t('context.removeConfirm.cancel')}
          </AlertDialogCancel>
          <AlertDialogAction
            onClick={(event) => {
              event.preventDefault();
              void confirm();
            }}
            className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            disabled={writeBlocked || submitting}
          >
            {t('context.removeConfirm.confirm')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
