import { memo, useCallback, useState } from 'react';
import { AlertTriangle, Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
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
import { useMutationStore } from '@/stores/mutation';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { executeSkillRemoval, openSkillRemoval } from '@/workflows/skill-remove';

export const DeleteSkillDialog = memo(function DeleteSkillDialog() {
  const { t } = useTranslation();
  const target = useSkillDialogStore((state) => state.deleteTarget);
  const preview = useSkillDialogStore((state) => state.deletePreview);
  const feedback = useSkillDialogStore((state) => state.deleteFeedback);
  const loading = useSkillDialogStore((state) => state.loadingAgentDetails);
  const close = useSkillDialogStore((state) => state.closeDelete);
  const writeBlocked = useMutationStore((state) => state.activeMutation !== null);
  const [removing, setRemoving] = useState(false);

  const confirm = useCallback(async () => {
    if (!preview) return;
    setRemoving(true);
    try {
      await executeSkillRemoval();
    } finally {
      setRemoving(false);
    }
  }, [preview]);

  const retryPreview = useCallback(async () => {
    if (!target) return;
    await openSkillRemoval(target.skill, target.context, target.projectPath);
  }, [target]);

  const retryingPreview = feedback === 'previewError' && !preview;
  const hasCopies = preview?.physicalEntries.some((entry) => entry.kind === 'directory') ?? false;

  return (
    <Dialog open={Boolean(target)} onOpenChange={(open) => !open && !removing && close()}>
      <DialogContent className="h-[min(30rem,calc(100dvh-2rem))] min-w-0 grid-rows-[auto_minmax(0,1fr)_auto] gap-0 overflow-hidden p-0 sm:max-w-lg">
        <DialogHeader className="min-w-0 px-6 pt-6 pb-4">
          <DialogTitle>{t('skills.deleteConfirm.title')}</DialogTitle>
          <DialogDescription className="min-w-0 break-words pr-6">
            {t('skills.deleteConfirm.description', { name: target?.skill.name })}
          </DialogDescription>
        </DialogHeader>

        <div
          data-testid="delete-skill-dialog-body"
          className="min-h-0 min-w-0 max-w-full space-y-4 overflow-y-auto overflow-x-hidden overscroll-contain px-6 pb-5"
        >
          {feedback ? (
            <div
              role="alert"
              className="flex min-w-0 gap-2 rounded-md border border-warning/30 bg-warning/10 p-3 text-sm"
            >
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-warning" aria-hidden="true" />
              <span className="min-w-0 break-words">
                {t(`skills.deleteConfirm.${feedback}`)}
              </span>
            </div>
          ) : null}

          {loading ? (
            <div className="space-y-2" role="status" aria-live="polite">
              <Skeleton className="h-16 w-full" />
              <Skeleton className="h-20 w-full" />
            </div>
          ) : (
            <>
              <section className="min-w-0 space-y-2">
                <h3 className="text-sm font-medium">{t('skills.deleteConfirm.sharedDirSection')}</h3>
                <code
                  className="block min-w-0 max-w-full text-xs leading-5 text-muted-foreground [overflow-wrap:anywhere]"
                  translate="no"
                >
                  {target?.skill.canonicalPath}
                </code>
              </section>

              {preview ? (
                <section className="min-w-0 space-y-2">
                  <h3 className="text-sm font-medium">
                    {t('skills.deleteConfirm.agentEntriesSection')}
                  </h3>
                  {preview.physicalEntries.length > 0 ? (
                    <div className="min-w-0 space-y-2">
                      {preview.physicalEntries.map((entry) => {
                        const mode = entry.kind === 'directory' ? 'copyMode' : 'linkMode';
                        return (
                          <div
                            key={entry.entryId}
                            className="min-w-0 max-w-full space-y-1.5 rounded-md border border-border/60 p-3"
                          >
                            <div className="flex min-w-0 max-w-full items-start gap-3">
                              <span className="min-w-0 flex-1 break-words text-sm font-medium">
                                {entry.owners.map((owner) => owner.displayName).join(', ')}
                              </span>
                              <Badge variant="secondary" className="shrink-0">
                                {t(`skills.deleteConfirm.${mode}`)}
                              </Badge>
                            </div>
                            <code
                              className="block min-w-0 max-w-full text-xs leading-5 text-muted-foreground [overflow-wrap:anywhere]"
                              translate="no"
                            >
                              {entry.displayPath.nativePath}
                            </code>
                          </div>
                        );
                      })}
                    </div>
                  ) : (
                    <p className="text-sm text-muted-foreground">
                      {t('skills.deleteConfirm.noAgentEntries')}
                    </p>
                  )}
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

        <DialogFooter className="min-w-0 border-t px-6 py-4">
          <Button variant="outline" onClick={close} disabled={removing}>
            {t('common.cancel')}
          </Button>
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
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
});
