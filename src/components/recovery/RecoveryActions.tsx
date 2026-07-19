import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ExternalLink, RefreshCw, Trash2 } from 'lucide-react';
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';
import { formatMutationError } from '@/lib/mutation-results';
import {
  confirmRecoveryResourceResolved,
  getRecoveryResourceStatus,
  openRecoveryResource,
} from '@/hooks/useTauriApi';
import type { RecoveryAction, RecoveryResourceStatus } from '@/bindings';

export function RecoveryActions({ recovery, initialStatus, onResolved }: {
  recovery: RecoveryAction;
  initialStatus?: RecoveryResourceStatus;
  onResolved?: () => void;
}) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<RecoveryResourceStatus | null>(() => (
    initialStatus?.resourceId === recovery.resourceId ? initialStatus : null
  ));
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(false);
  const [openError, setOpenError] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(false);
    try {
      setStatus(await getRecoveryResourceStatus(recovery.resourceId));
    } catch {
      setError(true);
    } finally {
      setLoading(false);
    }
  }, [recovery.resourceId]);

  useEffect(() => {
    if (initialStatus?.resourceId === recovery.resourceId) {
      setStatus(initialStatus);
      setError(false);
      return;
    }
    void refresh();
  }, [initialStatus, recovery.resourceId, refresh]);

  const cleanup = async () => {
    if (!status || status.state !== 'consistentCanCleanup') return;
    setLoading(true);
    try {
      await confirmRecoveryResourceResolved(recovery.resourceId, status.revision);
      setConfirmOpen(false);
      onResolved?.();
    } catch {
      setConfirmOpen(false);
      await refresh();
    } finally {
      setLoading(false);
    }
  };

  const open = async () => {
    setOpenError(false);
    try {
      await openRecoveryResource(recovery.resourceId);
    } catch {
      setOpenError(true);
    }
  };

  return (
    <div className="mt-3 space-y-2 rounded-md border border-warning/40 bg-warning/5 p-3" aria-live="assertive">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <p className="text-sm font-medium">{t('recovery.title')}</p>
          <p className="text-xs text-muted-foreground">
            {status ? t(`recovery.state.${status.state}`) : t('recovery.loading')}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" size="sm" disabled={loading} onClick={() => void open()}>
            <ExternalLink className="h-3.5 w-3.5" />{t('recovery.open')}
          </Button>
          <Button variant="outline" size="sm" disabled={loading} onClick={() => void refresh()}>
            <RefreshCw className="h-3.5 w-3.5" />{t('recovery.refresh')}
          </Button>
          {status?.state === 'consistentCanCleanup' ? (
            <Button variant="outline" size="sm" disabled={loading} onClick={() => setConfirmOpen(true)}>
              <Trash2 className="h-3.5 w-3.5" />{t('recovery.cleanup')}
            </Button>
          ) : null}
        </div>
      </div>
      {error ? <p className="text-xs text-destructive">{t('recovery.refreshError')}</p> : null}
      {openError ? <p className="text-xs text-destructive">{t('recovery.openError')}</p> : null}
      {status?.diagnostic ? (
        <p className="text-xs text-muted-foreground">
          {formatMutationError(status.diagnostic, t)}
        </p>
      ) : null}
      {status?.displayPaths.length ? (
        <div className="space-y-1">
          {status.displayPaths.map((path) => (
            <p key={`${path.environment.kind}:${path.nativePath}`} className="truncate font-mono text-[11px] text-muted-foreground" title={path.nativePath}>
              {path.nativePath}
            </p>
          ))}
        </div>
      ) : null}

      <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('recovery.cleanupTitle')}</AlertDialogTitle>
            <AlertDialogDescription>{t('recovery.cleanupDescription')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={loading}>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction disabled={loading} onClick={(event) => { event.preventDefault(); void cleanup(); }}>
              {t('recovery.confirmCleanup')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
