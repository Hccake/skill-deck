import { useTranslation } from 'react-i18next';
import { ArrowUpRight, LoaderCircle } from 'lucide-react';
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Button } from '@/components/ui/button';
import { LibraryUsageIdentity } from '@/components/library';
import { libraryUsageIdentityKey } from '@/lib/libraries/usage-presentation';
import type { AgentDeleteImpact, LibraryUsage } from '@/bindings';

type DeletePreviewState = 'loading' | 'ready' | 'error';

export function AgentDeleteDialog({
  target,
  impact,
  previewState,
  confirmation,
  deleting,
  writeBlocked,
  executionError,
  stale,
  onConfirmationChange,
  onClose,
  onConfirm,
  onRetryPreview,
  onOpenLibraryUsage,
}: {
  target: { agentId: string; displayName: string } | null;
  impact: AgentDeleteImpact | null;
  previewState: DeletePreviewState;
  confirmation: string;
  deleting: boolean;
  writeBlocked: boolean;
  executionError: boolean;
  stale: boolean;
  onConfirmationChange: (value: string) => void;
  onClose: () => void;
  onConfirm: () => void;
  onRetryPreview: () => void;
  onOpenLibraryUsage: (usage: LibraryUsage) => void;
}) {
  const { t } = useTranslation();
  const previewReady = previewState === 'ready' && impact !== null;
  return (
    <AlertDialog open={target !== null} onOpenChange={(open) => { if (!open && !deleting) onClose(); }}>
      <AlertDialogContent
        className="overscroll-contain"
        dismissible={!deleting}
        aria-busy={previewState === 'loading' || deleting}
      >
        <AlertDialogHeader>
          <AlertDialogTitle>{t('settings.agents.deleteTitle', { name: target?.displayName })}</AlertDialogTitle>
          <AlertDialogDescription>{t('settings.agents.deleteDescription')}</AlertDialogDescription>
        </AlertDialogHeader>
        <div className="min-h-32">
          {previewState === 'loading' ? (
            <div role="status" className="flex min-h-32 items-center justify-center gap-2 text-sm text-muted-foreground">
              <LoaderCircle className="size-4 animate-spin" aria-hidden="true" />
              {t('settings.agents.deletePreviewLoading')}
            </div>
          ) : null}
          {previewState === 'error' ? (
            <div className="flex min-h-32 items-center">
              <p role="alert" className="text-sm text-destructive">
                {t('settings.agents.deletePreviewError')}
              </p>
            </div>
          ) : null}
          {previewReady ? (
            <div className="max-h-[50vh] space-y-3 overflow-y-auto text-sm">
              {stale ? (
                <p role="alert" className="text-sm text-warning">
                  {t('settings.agents.deleteStale')}
                </p>
              ) : null}
              {executionError ? (
                <p role="alert" className="text-sm text-destructive">
                  {t('settings.agents.deleteError')}
                </p>
              ) : null}
              {impact.libraryUsages.length > 0 ? (
                <div className="border-y border-warning/40 py-3">
                  <p className="font-medium text-warning">{t('settings.agents.libraryUsageDeleteBlocked')}</p>
                  <div className="mt-2 flex flex-wrap gap-2">
                    {impact.libraryUsages.map((usage) => (
                      <Button
                        key={libraryUsageIdentityKey(usage)}
                        type="button"
                        variant="outline"
                        size="sm"
                        className="h-auto max-w-full justify-start gap-2 py-1.5"
                        onClick={() => onOpenLibraryUsage(usage)}
                      >
                        <LibraryUsageIdentity usage={usage} showPath={false} />
                        <ArrowUpRight className="size-3.5" aria-hidden="true" />
                      </Button>
                    ))}
                  </div>
                </div>
              ) : null}
              <p className="font-medium text-success">{t('settings.agents.deleteFilesSafe')}</p>
              {impact.scopes.map((scope) => (
                <section key={scope.scope} className="space-y-2 rounded-md border border-border/60 p-3">
                  <p className="font-medium">{t(`settings.agents.${scope.scope}.title`)}</p>
                  {scope.paths.map((path, index) => (
                    <div key={`${path.kind}:${index}`} className="space-y-1 text-xs">
                      <p className="text-muted-foreground">{t(`settings.agents.deletePathKind.${path.kind}`)}</p>
                      {path.resolvedPath ? (
                        <code className="block break-all font-mono text-[11px]" translate="no">{path.resolvedPath}</code>
                      ) : (
                        <p className="text-muted-foreground">
                          {path.unavailableReason
                            ? t(`settings.agents.deletePathUnavailableReasons.${path.unavailableReason}`)
                            : t('settings.agents.deletePathUnavailable')}
                        </p>
                      )}
                      {path.observedSkillCount !== null ? (
                        <p>{t('settings.agents.deleteObservedSkillCount', {
                          count: path.observedSkillCount,
                          suffix: path.observedSkillCountTruncated ? '+' : '',
                        })}</p>
                      ) : null}
                    </div>
                  ))}
                </section>
              ))}
              {impact.losesManagementCapability ? (
                <p className="text-warning">{t('settings.agents.deleteManagementWarning')}</p>
              ) : null}
              <div className="space-y-1.5">
                <Label htmlFor="delete-agent-confirmation">{t('settings.agents.deleteConfirmId')}</Label>
                <Input
                  id="delete-agent-confirmation"
                  name="delete-agent-confirmation"
                  value={confirmation}
                  autoComplete="off"
                  spellCheck={false}
                  disabled={deleting || writeBlocked}
                  onChange={(event) => onConfirmationChange(event.target.value)}
                />
                <p className="text-xs text-muted-foreground">
                  {t('settings.agents.deleteConfirmHint', { id: impact.agentId })}
                </p>
              </div>
            </div>
          ) : null}
        </div>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={deleting}>{t('common.cancel')}</AlertDialogCancel>
          {previewState === 'error' ? (
            <AlertDialogAction onClick={(event) => { event.preventDefault(); onRetryPreview(); }}>
              {t('settings.agents.retryDeletePreview')}
            </AlertDialogAction>
          ) : null}
          {previewReady ? (
            <AlertDialogAction
              disabled={writeBlocked || deleting || confirmation !== impact.agentId || impact.libraryUsages.length > 0}
              onClick={(event) => { event.preventDefault(); onConfirm(); }}
            >
              {t(deleting
                ? 'settings.agents.deleting'
                : executionError
                  ? 'settings.agents.retryDelete'
                  : 'settings.agents.confirmDelete')}
            </AlertDialogAction>
          ) : null}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
