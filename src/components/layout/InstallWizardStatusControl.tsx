import { useState } from 'react';
import { Loader2, LockKeyhole, TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';
import { continueInstallFlow } from '@/workflows/install-session-feedback';
import { cn } from '@/lib/utils';

export function InstallWizardStatusControl() {
  const { t } = useTranslation();
  const active = useInstallWizardSessionStore((state) => state.active);
  const syncError = useInstallWizardSessionStore((state) => state.syncError);
  const refreshSession = useInstallWizardSessionStore((state) => state.refreshSession);
  const retryMonitoring = useInstallWizardSessionStore((state) => state.retryMonitoring);
  const [busy, setBusy] = useState(false);

  if (!active && syncError === null) return null;

  const unavailable = syncError !== null;
  const labelKey = unavailable
    ? 'installWizardSession.syncFailedTitle'
    : 'installWizardSession.writeUnavailable';
  const descriptionKey = unavailable
    ? 'installWizardSession.syncFailedDescription'
    : 'installWizardSession.writeUnavailableDescription';
  const StatusIcon = unavailable ? TriangleAlert : LockKeyhole;

  const activate = async () => {
    if (busy) return;
    if (syncError === 'monitor') {
      retryMonitoring();
      return;
    }

    setBusy(true);
    try {
      if (syncError === 'refresh') {
        await refreshSession();
      } else {
        await continueInstallFlow();
      }
    } catch (error) {
      console.error('Failed to activate install session status control:', error);
      if (!unavailable) {
        toast.error(t('installWizardSession.focusFailed'));
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className={cn(
            'h-8 min-w-8 shrink-0 gap-1.5 px-2 sm:h-9',
            unavailable
              ? 'text-amber-700 hover:text-amber-800 dark:text-amber-300 dark:hover:text-amber-200'
              : 'text-muted-foreground hover:text-foreground',
          )}
          aria-label={t(labelKey)}
          disabled={busy}
          onClick={() => void activate()}
        >
          {busy ? (
            <Loader2 className="animate-spin motion-reduce:animate-none" aria-hidden="true" />
          ) : (
            <StatusIcon aria-hidden="true" />
          )}
          <span className="hidden max-w-36 truncate text-xs xl:inline">
            {t(labelKey)}
          </span>
        </Button>
      </TooltipTrigger>
      <TooltipContent className="max-w-72">
        {t(descriptionKey)}
      </TooltipContent>
    </Tooltip>
  );
}
