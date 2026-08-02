import { useState } from 'react';
import { LockKeyhole, Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { focusInstallWizard } from '@/hooks/useTauriApi';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';

export function InstallWizardReadOnlyBar() {
  const { t } = useTranslation();
  const active = useInstallWizardSessionStore((state) => state.active);
  const loading = useInstallWizardSessionStore((state) => state.loading);
  const syncError = useInstallWizardSessionStore((state) => state.syncError);
  const refreshSession = useInstallWizardSessionStore((state) => state.refreshSession);
  const retryMonitoring = useInstallWizardSessionStore((state) => state.retryMonitoring);
  const [focusing, setFocusing] = useState(false);
  const [retrying, setRetrying] = useState(false);

  if (!active && !loading && !syncError) return null;

  const returnToWizard = async () => {
    if (focusing) return;
    setFocusing(true);
    try {
      if (!await focusInstallWizard()) {
        await refreshSession();
      }
    } catch (error) {
      console.error('Failed to focus install wizard:', error);
      toast.error(t('installWizardSession.focusFailed'));
    } finally {
      setFocusing(false);
    }
  };

  const retrySync = async () => {
    if (retrying) return;
    if (syncError === 'monitor') {
      retryMonitoring();
      return;
    }

    setRetrying(true);
    try {
      await refreshSession();
    } catch (error) {
      console.error('Failed to retry install wizard session sync:', error);
    } finally {
      setRetrying(false);
    }
  };

  const title = syncError
    ? t('installWizardSession.syncFailedTitle')
    : active
      ? t('installWizardSession.readOnlyTitle')
      : t('installWizardSession.checkingTitle');
  const description = syncError
    ? t('installWizardSession.syncFailedDescription')
    : active
      ? t('installWizardSession.readOnlyDescription')
      : t('installWizardSession.checkingDescription');

  return (
    <div
      role="status"
      aria-live="polite"
      className="flex flex-shrink-0 items-center gap-3 border-b border-amber-500/30 bg-amber-500/10 px-3 py-2 text-amber-950 dark:text-amber-100 sm:px-4"
    >
      <LockKeyhole
        className="h-4 w-4 flex-shrink-0 text-amber-600 dark:text-amber-400"
        aria-hidden="true"
      />
      <div className="min-w-0 flex-1">
        <p className="text-xs font-semibold sm:text-sm">
          {title}
        </p>
        <p className="text-[11px] leading-4 text-amber-900/75 dark:text-amber-100/70 sm:text-xs">
          {description}
        </p>
      </div>
      {active || syncError ? (
        <div className="flex flex-shrink-0 items-center gap-1.5">
          {active ? (
            <Button
              type="button"
              size="sm"
              variant="outline"
              className="h-7 flex-shrink-0 border-amber-500/40 bg-background/80 px-2.5 text-xs hover:bg-background"
              disabled={focusing}
              onClick={() => void returnToWizard()}
            >
              {focusing ? <Loader2 className="animate-spin motion-reduce:animate-none" aria-hidden="true" /> : null}
              {t('installWizardSession.returnToWizard')}
            </Button>
          ) : null}
          {syncError ? (
            <Button
              type="button"
              size="sm"
              variant="outline"
              className="h-7 flex-shrink-0 border-amber-500/40 bg-background/80 px-2.5 text-xs hover:bg-background"
              disabled={retrying}
              onClick={() => void retrySync()}
            >
              {retrying ? <Loader2 className="animate-spin motion-reduce:animate-none" aria-hidden="true" /> : null}
              {t('installWizardSession.retryMonitoring')}
            </Button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
