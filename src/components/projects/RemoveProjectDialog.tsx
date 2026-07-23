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
  const [failedRequest, setFailedRequest] = useState<ProjectRemovalRequest | null>(null);
  const failed = failedRequest === request;

  const confirm = async () => {
    if (!request || writeBlocked || submitting) return;
    const completedRequest = request;
    setFailedRequest(null);
    setSubmitting(true);
    try {
      await confirmProjectRemoval(completedRequest);
      onClose();
      onRemoved?.(completedRequest);
    } catch (error) {
      setFailedRequest(completedRequest);
      console.error('Failed to remove project:', error);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <AlertDialog
      open={request !== null}
      onOpenChange={(open) => {
        if (!open && !submitting) {
          setFailedRequest(null);
          onClose();
        }
      }}
    >
      <AlertDialogContent dismissible={!submitting} aria-busy={submitting}>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('context.removeConfirm.title')}</AlertDialogTitle>
          <AlertDialogDescription className="space-y-2">
            <span className="block">
              {t('context.removeConfirm.description', { name: request?.projectName ?? '' })}
            </span>
            <span className="block">{t('context.removeConfirm.unregisterOnly')}</span>
          </AlertDialogDescription>
        </AlertDialogHeader>
        {failed ? (
          <p role="alert" className="text-sm text-destructive">
            {t('context.removeConfirm.removeError')}
          </p>
        ) : null}
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
            {t(submitting
              ? 'context.removeConfirm.removing'
              : failed
                ? 'context.removeConfirm.retry'
                : 'context.removeConfirm.confirm')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
