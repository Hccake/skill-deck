import { useCallback, useEffect, useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  AlertTriangle,
  CheckCircle2,
  FileWarning,
  FolderOpen,
  RefreshCw,
  Trash2,
  WifiOff,
} from 'lucide-react';
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
import type {
  MutationKind,
  RecoveryAction,
  RecoveryResourceState,
  RecoveryResourceStatus,
} from '@/bindings';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import { runBusinessWrite } from '@/workflows/install-session-feedback';
import { environmentKey, useEnvironmentStore } from '@/stores/environment';
import {
  environmentDisplayName,
  environmentRefDisplayName,
} from '@/lib/environments/presentation';

const OPERATION_TITLE_KINDS = new Set<MutationKind>([
  'install',
  'update',
  'remove',
  'copy',
  'manageAgents',
  'duplicateCleanup',
  'repair',
]);

function statusIcon(state: RecoveryResourceState | undefined) {
  switch (state) {
    case 'consistentCanCleanup':
      return <CheckCircle2 className="mt-0.5 h-5 w-5 shrink-0 text-success" aria-hidden="true" />;
    case 'environmentUnavailable':
      return <WifiOff className="mt-0.5 h-5 w-5 shrink-0 text-warning" aria-hidden="true" />;
    case 'invalid':
      return <FileWarning className="mt-0.5 h-5 w-5 shrink-0 text-destructive" aria-hidden="true" />;
    default:
      return <AlertTriangle className="mt-0.5 h-5 w-5 shrink-0 text-warning" aria-hidden="true" />;
  }
}

export function RecoveryActions({ recovery, initialStatus, onResolved }: {
  recovery: RecoveryAction;
  initialStatus?: RecoveryResourceStatus;
  onResolved?: () => void;
}) {
  const { t, i18n } = useTranslation();
  const environments = useEnvironmentStore((store) => store.environments);
  const titleId = useId();
  const writeBlocked = useBusinessWriteBlocked();
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
    if (writeBlocked || !status || status.state !== 'consistentCanCleanup') return;
    setLoading(true);
    try {
      const outcome = await runBusinessWrite(() => (
        confirmRecoveryResourceResolved(recovery.resourceId, status.revision)
      ));
      setConfirmOpen(false);
      if (outcome.status === 'notRun') return;
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

  const subject = status?.subject;
  const title = subject
    ? t(
        OPERATION_TITLE_KINDS.has(subject.operationKind)
          ? `recovery.itemTitle.${subject.operationKind}`
          : 'recovery.itemTitle.generic',
        { skillName: subject.skillName },
      )
    : t('recovery.title');
  const subjectEnvironment = subject?.context.environment;
  const environment = subjectEnvironment
    ? environments.find((entry) => (
      environmentKey(entry.environment) === environmentKey(subjectEnvironment)
    ))
    : undefined;
  const environmentLabel = subjectEnvironment
    ? environment
      ? environmentDisplayName(environment, t)
      : environmentRefDisplayName(subjectEnvironment, undefined, t)
    : '';
  const scopeLabel = subject?.context.scope.scope === 'project'
    ? t('recovery.projectScope')
    : t('context.global');
  const createdAt = status && status.createdAtEpochMs > 0
    ? new Intl.DateTimeFormat(i18n.language, {
        dateStyle: 'medium',
        timeStyle: 'short',
      }).format(new Date(status.createdAtEpochMs))
    : null;
  const metadata = subject
    ? [scopeLabel, environmentLabel, createdAt].filter(Boolean).join(' · ')
    : createdAt;

  return (
    <section
      className="rounded-md border border-warning/40 bg-warning/5 p-4"
      aria-labelledby={titleId}
      aria-busy={loading}
    >
      <div className="flex items-start gap-3">
        {statusIcon(status?.state)}
        <div className="min-w-0 flex-1">
          <h3 id={titleId} className="text-sm font-semibold text-foreground">
            {title}
          </h3>
          {metadata ? (
            <p className="mt-0.5 text-xs text-muted-foreground">{metadata}</p>
          ) : null}
          <div className="mt-2" role="status" aria-live="polite">
            <p className="text-sm text-muted-foreground">
              {status ? t(`recovery.state.${status.state}`) : t('recovery.loading')}
            </p>
          </div>
        </div>
      </div>

      {status?.diagnostic ? (
        <p className="mt-3 text-xs text-destructive" role="alert">
          {formatMutationError(status.diagnostic, t)}
        </p>
      ) : null}

      {status?.paths.length ? (
        <div className="mt-4 divide-y divide-border overflow-hidden rounded-md border bg-background">
          {status.paths.map((path, index) => (
            <div
              key={`${path.kind}:${path.location.environment.kind}:${path.location.nativePath}:${index}`}
              className="px-3 py-2.5"
            >
              <p className="text-xs font-medium text-foreground">{t(`recovery.path.${path.kind}`)}</p>
              <p className="mt-1 break-all font-mono text-xs text-muted-foreground">
                {path.location.nativePath}
              </p>
            </div>
          ))}
        </div>
      ) : null}

      {error ? <p className="mt-3 text-xs text-destructive" role="alert">{t('recovery.refreshError')}</p> : null}
      {openError ? <p className="mt-3 text-xs text-destructive" role="alert">{t('recovery.openError')}</p> : null}

      <div className="mt-4 flex flex-wrap justify-end gap-2 border-t border-border/70 pt-3">
        <Button
          variant={status?.state === 'needsAttention' ? 'default' : 'outline'}
          size="sm"
          disabled={loading}
          onClick={() => void open()}
        >
          <FolderOpen className="h-3.5 w-3.5" aria-hidden="true" />
          {t(status?.state === 'invalid' ? 'recovery.openRecordDirectory' : 'recovery.openDirectory')}
        </Button>
        <Button variant="outline" size="sm" disabled={loading} onClick={() => void refresh()}>
          <RefreshCw
            className={`h-3.5 w-3.5${loading ? ' animate-spin motion-reduce:animate-none' : ''}`}
            aria-hidden="true"
          />
          {t('recovery.refresh')}
        </Button>
        {status?.state === 'consistentCanCleanup' ? (
          <Button
            variant="destructive"
            size="sm"
            disabled={writeBlocked || loading}
            onClick={() => setConfirmOpen(true)}
          >
            <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
            {t('recovery.cleanup')}
          </Button>
        ) : null}
      </div>

      <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <AlertDialogContent dismissible={!loading} aria-busy={loading}>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('recovery.cleanupTitle')}</AlertDialogTitle>
            <AlertDialogDescription>{t('recovery.cleanupDescription')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={loading}>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction
              disabled={writeBlocked || loading}
              onClick={(event) => { event.preventDefault(); void cleanup(); }}
            >
              {t('recovery.confirmCleanup')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  );
}
