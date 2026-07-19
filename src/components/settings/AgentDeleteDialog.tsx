import { useTranslation } from 'react-i18next';
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import type { AgentDeleteImpact } from '@/bindings';

export function AgentDeleteDialog({
  impact,
  confirmation,
  deleting,
  onConfirmationChange,
  onClose,
  onConfirm,
}: {
  impact: AgentDeleteImpact | null;
  confirmation: string;
  deleting: boolean;
  onConfirmationChange: (value: string) => void;
  onClose: () => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  return (
    <AlertDialog open={impact !== null} onOpenChange={(open) => { if (!open && !deleting) onClose(); }}>
      <AlertDialogContent className="overscroll-contain">
        <AlertDialogHeader>
          <AlertDialogTitle>{t('settings.agents.deleteTitle', { name: impact?.displayName })}</AlertDialogTitle>
          <AlertDialogDescription>{t('settings.agents.deleteDescription')}</AlertDialogDescription>
        </AlertDialogHeader>
        <div className="max-h-[50vh] space-y-3 overflow-y-auto text-sm">
          <p className="font-medium text-success">{t('settings.agents.deleteFilesSafe')}</p>
          {impact?.scopes.map((scope) => (
            <section key={scope.scope} className="space-y-2 rounded-md border border-border/60 p-3">
              <div className="flex items-center justify-between gap-3">
                <p className="font-medium">{t(`settings.agents.${scope.scope}.title`)}</p>
                {scope.defaultReferenced ? (
                  <span className="text-xs text-warning">{t('settings.agents.deleteDefaultReferenced')}</span>
                ) : null}
              </div>
              {scope.paths.map((path, index) => (
                <div key={`${path.kind}:${index}`} className="space-y-1 text-xs">
                  <p className="text-muted-foreground">{t(`settings.agents.deletePathKind.${path.kind}`)}</p>
                  {path.resolvedPath ? (
                    <code className="block break-all font-mono text-[11px]" translate="no">{path.resolvedPath}</code>
                  ) : (
                    <p className="text-muted-foreground">{t('settings.agents.deletePathUnavailable')}</p>
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
          {impact?.losesManagementCapability ? (
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
              disabled={deleting}
              onChange={(event) => onConfirmationChange(event.target.value)}
            />
            <p className="text-xs text-muted-foreground">
              {t('settings.agents.deleteConfirmHint', { id: impact?.agentId })}
            </p>
          </div>
        </div>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={deleting}>{t('common.cancel')}</AlertDialogCancel>
          <AlertDialogAction
            disabled={deleting || confirmation !== impact?.agentId}
            onClick={(event) => { event.preventDefault(); onConfirm(); }}
          >
            {t('settings.agents.confirmDelete')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
