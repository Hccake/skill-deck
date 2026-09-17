import { memo, useCallback, useEffect, useRef, useState } from 'react';
import { AlertTriangle, Files, Link2, Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { RecoveryActions } from '@/components/recovery/RecoveryActions';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Skeleton } from '@/components/ui/skeleton';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import { useProjectWorkspace } from '@/hooks/useProjectWorkspace';
import { projectDisplayName } from '@/lib/projects/presentation';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { executeSkillRemoval, openSkillRemoval } from '@/workflows/skill-remove';
import type { RecoveryAction } from '@/bindings';
import { InstallPath } from './InstallPath';
import './DeleteSkillDialog.css';

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
  const cancelButtonRef = useRef<HTMLButtonElement>(null);
  const { projects } = useProjectWorkspace(target?.context.environment ?? { kind: 'native' });
  const scope = target?.context.scope;
  const project = scope?.scope === 'project'
    ? projects.find((item) => item.binding.id === scope.project_id)
    : undefined;
  const scopeLabel = scope?.scope === 'project'
    ? project ? t('skills.installPath.project', { name: projectDisplayName(project) }) : t('context.projects')
    : t('context.global');

  useEffect(() => {
    setRecovery([]);
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

  return (
    <Dialog open={Boolean(target)} onOpenChange={(open) => !open && !removing && close()}>
      <DialogContent
        className="delete-skill-dialog max-h-[calc(100dvh-2rem)] min-w-0 grid-rows-[auto_minmax(0,1fr)_auto] gap-0 overflow-hidden p-0"
        dismissible={!removing}
        closeLabel={t('common.close')}
        aria-busy={loading || removing}
        aria-describedby={undefined}
        onOpenAutoFocus={(event) => {
          event.preventDefault();
          cancelButtonRef.current?.focus();
        }}
      >
        <DialogHeader className="min-h-14 min-w-0 justify-center border-b py-3 pl-5 pr-12 text-left">
          <DialogTitle className="min-w-0 text-base leading-6 [overflow-wrap:anywhere]">
            <span>{t('skills.deleteConfirm.title', { name: target?.skill.name ?? '' })}</span>{' '}
            <span className="inline-block max-w-full text-xs font-normal text-muted-foreground">· {scopeLabel}</span>
          </DialogTitle>
        </DialogHeader>

        <div
          data-testid="delete-skill-dialog-body"
          className="min-h-0 min-w-0 max-w-full space-y-3 overflow-y-auto overflow-x-hidden overscroll-contain px-5 py-4"
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
          ) : preview ? (
            <ul
              data-testid="delete-skill-entry-list"
              className="delete-skill-positions"
              aria-label={t('skills.deleteConfirm.scopeLabel')}
            >
              {preview.standardPath ? <li
                data-testid="delete-skill-entry"
                className="delete-skill-position"
              >
                <span className="delete-skill-position-name">{t('skills.deleteConfirm.standardDirSection')}</span>
                <div className="delete-skill-position-path">
                  <InstallPath path={preview.standardPath} base={preview.pathBase} scope={preview.context.scope.scope} copyable={false} />
                </div>
              </li> : null}

              {preview.physicalEntries.map((entry) => {
                const copy = entry.kind === 'directory';
                const mode = copy ? 'copyMode' : 'linkMode';
                const EntryIcon = copy ? Files : Link2;
                return (
                  <li
                    key={entry.entryId}
                    data-testid="delete-skill-entry"
                    className="delete-skill-position"
                  >
                    <span className="delete-skill-position-name">
                      {entry.readers.map((reader) => reader.displayName).join(' · ') || t('skills.updatePlan.installLocation')}
                    </span>
                    <div className="delete-skill-position-path">
                      <InstallPath path={entry.displayPath} base={preview.pathBase} scope={preview.context.scope.scope} copyable={false} />
                    </div>
                    <span className="delete-skill-position-mode text-muted-foreground">
                      <EntryIcon className="size-3.5 shrink-0" aria-hidden="true" />
                      {t(`skills.deleteConfirm.${mode}`)}
                    </span>
                  </li>
                );
              })}
            </ul>
          ) : null}
        </div>

        <DialogFooter className="min-h-14 min-w-0 flex-row flex-wrap items-center justify-end gap-3 border-t px-5 py-2.5">
          {!loading && !recoveryRequired && preview && (hasCopies || preview.restoresLibrary) ? (
            <div className="mr-auto min-w-0 flex-1 basis-48 space-y-0.5 text-xs text-muted-foreground">
              {hasCopies ? <p>{t('skills.deleteConfirm.copyWarning')}</p> : null}
              {preview.restoresLibrary ? <p>{t('skills.deleteConfirm.restoresLibrary')}</p> : null}
            </div>
          ) : null}
          <div className="ml-auto flex shrink-0 items-center gap-3">
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
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
});
