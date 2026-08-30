import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
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
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import { formatAppError } from '@/utils/format-app-error';
import {
  confirmLibraryDeletion,
  type LibraryDeletionRequest,
} from '@/workflows/library-deletion';
import type { AppError } from '@/bindings';

interface DeleteLibraryDialogProps {
  request: LibraryDeletionRequest | null;
  onClose: () => void;
}

export function DeleteLibraryDialog({ request, onClose }: DeleteLibraryDialogProps) {
  const { t } = useTranslation();
  const writeBlocked = useBusinessWriteBlocked();
  const [submitting, setSubmitting] = useState(false);
  const [failure, setFailure] = useState<{
    request: LibraryDeletionRequest;
    error: AppError;
  } | null>(null);
  const currentFailure = failure?.request === request ? failure.error : null;
  const failureMessage = currentFailure?.kind === 'validation'
    && currentFailure.data.field === 'libraryId'
    ? t('libraries.lockedDelete')
    : currentFailure
      ? formatAppError(currentFailure, t)
      : null;

  const confirm = async () => {
    if (!request || submitting || writeBlocked) return;
    const submittedRequest = request;
    setFailure(null);
    setSubmitting(true);
    try {
      const result = await confirmLibraryDeletion(submittedRequest);
      if (result.status === 'deleted') {
        onClose();
      } else if (result.status === 'stale') {
        toast.info(t('libraries.deleteLibraryAlreadyMissing'));
        onClose();
      } else if (result.status === 'failed') {
        setFailure({ request: submittedRequest, error: result.error });
      }
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <AlertDialog
      open={request !== null}
      onOpenChange={(open) => {
        if (!open && !submitting) {
          setFailure(null);
          onClose();
        }
      }}
    >
      <AlertDialogContent dismissible={!submitting} aria-busy={submitting}>
        <AlertDialogHeader>
          <AlertDialogTitle>
            {t('libraries.deleteLibraryTitle', { name: request?.libraryName ?? '' })}
          </AlertDialogTitle>
          <AlertDialogDescription className="space-y-2">
            <span className="block">
              {t('libraries.deleteLibraryDescriptionWithCount', {
                count: request?.skillCount ?? 0,
              })}
            </span>
            <span className="block">{t('libraries.deleteLibraryIrreversible')}</span>
          </AlertDialogDescription>
        </AlertDialogHeader>
        {failureMessage ? (
          <p role="alert" className="text-sm text-destructive">
            {failureMessage}
          </p>
        ) : null}
        <AlertDialogFooter>
          <AlertDialogCancel disabled={submitting}>{t('common.cancel')}</AlertDialogCancel>
          <AlertDialogAction
            variant="destructive"
            disabled={submitting || writeBlocked}
            onClick={(event) => {
              event.preventDefault();
              void confirm();
            }}
          >
            {t(submitting
              ? 'libraries.deletingLibrary'
              : currentFailure
                ? 'libraries.retryDeleteLibrary'
                : 'common.delete')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
