import { memo, useCallback, useEffect, useRef, useState } from 'react';
import { AlertTriangle, Copy, Folder, Link2, Loader2, Trash2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { RecoveryActions } from '@/components/recovery/RecoveryActions';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Skeleton } from '@/components/ui/skeleton';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { executeSkillRemoval, openSkillRemoval } from '@/workflows/skill-remove';
import type { RecoveryAction } from '@/bindings';

function getSkillBasePath(canonicalPath: string): string | null {
  const trimmedPath = canonicalPath.replace(/[\\/]+$/, '');
  const normalizedPath = trimmedPath.replace(/\\/g, '/');
  const windowsPath = /^[A-Za-z]:\//.test(normalizedPath) || normalizedPath.startsWith('//');
  const comparablePath = windowsPath ? normalizedPath.toLowerCase() : normalizedPath;
  const sharedDirectoryMarker = '/.agents/skills/';
  const markerIndex = comparablePath.lastIndexOf(sharedDirectoryMarker);

  if (markerIndex < 0 || comparablePath.slice(markerIndex + sharedDirectoryMarker.length).includes('/')) {
    return null;
  }

  const basePath = trimmedPath.slice(0, markerIndex);
  if (basePath) return /^[A-Za-z]:$/.test(basePath) ? `${basePath}\\` : basePath;
  return normalizedPath.startsWith('/') ? '/' : null;
}

function relativeToBase(path: string, basePath: string | null): string {
  if (!basePath) return path;

  const normalizedPath = path.replace(/\\/g, '/');
  const normalizedBase = basePath.replace(/\\/g, '/').replace(/\/+$/, '');
  const prefix = normalizedBase ? `${normalizedBase}/` : '/';
  const windowsPath = /^[A-Za-z]:\//.test(normalizedPath) || normalizedPath.startsWith('//');
  const comparablePath = windowsPath ? normalizedPath.toLowerCase() : normalizedPath;
  const comparablePrefix = windowsPath ? prefix.toLowerCase() : prefix;

  return comparablePath.startsWith(comparablePrefix) ? path.slice(prefix.length) : path;
}

function PathModeButton({
  active,
  children,
  onClick,
}: {
  active: boolean;
  children: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={`h-7 rounded-[6px] px-3 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50 ${active ? 'bg-background text-foreground shadow-xs' : 'text-muted-foreground hover:text-foreground'}`}
    >
      {children}
    </button>
  );
}

export const DeleteSkillDialog = memo(function DeleteSkillDialog() {
  const { t } = useTranslation();
  const target = useSkillDialogStore((state) => state.deleteTarget);
  const preview = useSkillDialogStore((state) => state.deletePreview);
  const feedback = useSkillDialogStore((state) => state.deleteFeedback);
  const loading = useSkillDialogStore((state) => state.loadingAgentDetails);
  const close = useSkillDialogStore((state) => state.closeDelete);
  const writeBlocked = useBusinessWriteBlocked();
  const [removing, setRemoving] = useState(false);
  const [recovery, setRecovery] = useState<RecoveryAction[]>([]);
  const [showFullPaths, setShowFullPaths] = useState(false);
  const cancelButtonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    setRecovery([]);
    setShowFullPaths(false);
  }, [preview, target]);

  const confirm = useCallback(async () => {
    if (!preview) return;
    setRecovery([]);
    setRemoving(true);
    try {
      const outcome = await executeSkillRemoval();
      if (outcome.status === 'recoveryRequired') setRecovery(outcome.recovery);
    } finally {
      setRemoving(false);
    }
  }, [preview]);

  const retryPreview = useCallback(async () => {
    if (!target) return;
    await openSkillRemoval(target.skill, target.context, target.projectPath);
  }, [target]);

  const retryingPreview = feedback === 'previewError' && !preview;
  const recoveryRequired = recovery.length > 0;
  const hasCopies = preview?.physicalEntries.some((entry) => entry.kind === 'directory') ?? false;
  const canonicalPath = target?.skill.canonicalPath ?? '';
  const basePath = target ? getSkillBasePath(canonicalPath) : null;
  const relativeSharedPath = relativeToBase(canonicalPath, basePath);
  const sharedDisplayPath = showFullPaths ? canonicalPath : relativeSharedPath;
  const canToggleFullPaths = Boolean(basePath && relativeSharedPath !== canonicalPath);

  return (
    <Dialog open={Boolean(target)} onOpenChange={(open) => !open && !removing && close()}>
      <DialogContent
        className="max-h-[calc(100dvh-2rem)] min-w-0 grid-rows-[auto_minmax(0,1fr)_auto] gap-0 overflow-hidden p-0 sm:max-w-[580px]"
        dismissible={!removing}
        closeLabel={t('common.close')}
        aria-busy={loading || removing}
        onOpenAutoFocus={(event) => {
          event.preventDefault();
          cancelButtonRef.current?.focus();
        }}
      >
        <DialogHeader className="min-w-0 px-6 pt-6 pb-5">
          <div className="flex min-w-0 items-start gap-3 pr-6 text-left">
            <div className="flex size-9 shrink-0 items-center justify-center rounded-md bg-destructive/10 text-destructive">
              <Trash2 className="size-4" aria-hidden="true" />
            </div>
            <div className="min-w-0 flex-1">
              <DialogTitle>{t('skills.deleteConfirm.title')}</DialogTitle>
              <DialogDescription className="mt-1 min-w-0 break-words">
                {t('skills.deleteConfirm.description', { name: target?.skill.name })}
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <div
          data-testid="delete-skill-dialog-body"
          className="min-h-0 min-w-0 max-w-full space-y-4 overflow-y-auto overflow-x-hidden overscroll-contain px-6 pb-5"
        >
          {feedback || recoveryRequired ? (
            <div
              role="alert"
              className="flex min-w-0 gap-2 rounded-md border border-warning/30 bg-warning/10 p-3 text-sm"
            >
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-warning" aria-hidden="true" />
              <div className="min-w-0 flex-1 break-words">
                <p>
                  {recoveryRequired
                    ? t('skills.deleteConfirm.recoveryRequired')
                    : t(`skills.deleteConfirm.${feedback}`)}
                </p>
                {recovery.map((action) => (
                  <RecoveryActions
                    key={action.resourceId}
                    recovery={action}
                    onResolved={close}
                  />
                ))}
              </div>
            </div>
          ) : null}

          {loading ? (
            <div className="space-y-2" role="status" aria-live="polite">
              <Skeleton className="h-16 w-full" />
              <Skeleton className="h-20 w-full" />
            </div>
          ) : (
            <>
              {preview ? (
                <section
                  className="min-w-0"
                  aria-label={t('skills.deleteConfirm.scopeLabel')}
                >
                  <div
                    data-testid="delete-skill-scope-summary"
                    className="mb-3 flex flex-wrap items-center justify-between gap-3"
                  >
                    <div className="flex items-baseline gap-2">
                      <h3 className="text-sm font-semibold">
                        {t('skills.deleteConfirm.scopeLabel')}
                      </h3>
                      <span className="text-xs text-muted-foreground">
                        {t('skills.deleteConfirm.scopeCount', {
                          count: preview.physicalEntries.length + 1,
                        })}
                      </span>
                    </div>

                    {canToggleFullPaths ? (
                      <div
                        role="group"
                        aria-label={t('skills.deleteConfirm.pathDisplayMode')}
                        className="inline-flex rounded-md bg-secondary p-0.5"
                      >
                        <PathModeButton
                          active={!showFullPaths}
                          onClick={() => setShowFullPaths(false)}
                        >
                          {t('skills.deleteConfirm.relativePaths')}
                        </PathModeButton>
                        <PathModeButton
                          active={showFullPaths}
                          onClick={() => setShowFullPaths(true)}
                        >
                          {t('skills.deleteConfirm.fullPaths')}
                        </PathModeButton>
                      </div>
                    ) : null}
                  </div>

                  {preview.restoresLibrary ? (
                    <div className="mb-3 flex items-start gap-2 border-y border-border/60 py-3 text-sm">
                      <Link2 className="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
                      <span>{t('skills.deleteConfirm.restoresLibrary')}</span>
                    </div>
                  ) : null}

                  <div
                    data-testid="delete-skill-entry-list"
                    className="min-w-0 divide-y divide-border/60 border-y border-border/60"
                  >
                    <div
                      data-testid="delete-skill-entry"
                      className="flex min-w-0 items-start gap-3 py-3"
                    >
                      <div className="mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-md bg-secondary text-muted-foreground">
                        <Folder className="size-3.5" aria-hidden="true" />
                      </div>
                      <div className="min-w-0 flex-1">
                        <div className="flex min-w-0 items-start justify-between gap-3">
                          <span className="min-w-0 flex-1 break-words text-sm font-medium">
                            {t('skills.deleteConfirm.standardDirSection')}
                          </span>
                          <span className="shrink-0 text-xs text-muted-foreground">
                            {t('skills.deleteConfirm.mainDirectory')}
                          </span>
                        </div>
                        <code
                          className="mt-1 block min-w-0 max-w-full text-xs leading-5 text-muted-foreground [overflow-wrap:anywhere]"
                          translate="no"
                        >
                          {sharedDisplayPath}
                        </code>
                      </div>
                    </div>

                    {preview.physicalEntries.map((entry) => {
                      const copy = entry.kind === 'directory';
                      const mode = copy ? 'copyMode' : 'linkMode';
                      const EntryIcon = copy ? Copy : Link2;
                      const displayPath = showFullPaths
                        ? entry.displayPath.nativePath
                        : relativeToBase(entry.displayPath.nativePath, basePath);
                      return (
                        <div
                          key={entry.entryId}
                          data-testid="delete-skill-entry"
                          className="flex min-w-0 max-w-full items-start gap-3 py-3"
                        >
                          <div className="mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-md bg-secondary text-muted-foreground">
                            <EntryIcon className="size-3.5" aria-hidden="true" />
                          </div>
                          <div className="min-w-0 flex-1">
                            <div className="flex min-w-0 max-w-full items-start gap-3">
                              <span className="min-w-0 flex-1 break-words text-sm font-medium">
                                {entry.readers.map((reader) => reader.displayName).join(', ')}
                              </span>
                              <Badge
                                variant="secondary"
                                className={`shrink-0 ${copy ? 'bg-warning/12 text-warning' : ''}`}
                              >
                                {t(`skills.deleteConfirm.${mode}`)}
                              </Badge>
                            </div>
                            <code
                              className="mt-1 block min-w-0 max-w-full text-xs leading-5 text-muted-foreground [overflow-wrap:anywhere]"
                              translate="no"
                            >
                              {displayPath}
                            </code>
                          </div>
                        </div>
                      );
                    })}
                  </div>

                </section>
              ) : null}

              {hasCopies ? (
                <div className="flex min-w-0 gap-2 rounded-md border border-warning/30 bg-warning/10 p-3 text-sm">
                  <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-warning" aria-hidden="true" />
                  <span className="min-w-0 break-words">
                    {t('skills.deleteConfirm.copyWarning')}
                  </span>
                </div>
              ) : null}
            </>
          )}
        </div>

        <DialogFooter className="min-w-0 flex-row justify-end border-t px-6 py-4">
          <Button ref={cancelButtonRef} variant="outline" onClick={close} disabled={removing}>
            {t(recoveryRequired ? 'common.close' : 'common.cancel')}
          </Button>
          {!recoveryRequired ? (
            <Button
              variant="destructive"
              onClick={retryingPreview ? retryPreview : confirm}
              disabled={writeBlocked || removing || loading || (!preview && !retryingPreview)}
            >
              {removing ? <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" /> : null}
              {retryingPreview
                ? t('skills.deleteConfirm.retryPreview')
                : feedback === 'executionError'
                  ? t('skills.deleteConfirm.retryDelete')
                  : t('skills.deleteConfirm.confirm')}
            </Button>
          ) : null}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
});
