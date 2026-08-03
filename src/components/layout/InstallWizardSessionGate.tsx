import { useEffect, useState, type ReactNode } from 'react';
import { Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';

const STARTUP_FEEDBACK_DELAY_MS = 300;

interface InstallWizardSessionGateProps {
  children: ReactNode;
}

function StartupFeedback() {
  const { t } = useTranslation();
  const [showFeedback, setShowFeedback] = useState(false);

  useEffect(() => {
    const timer = window.setTimeout(() => setShowFeedback(true), STARTUP_FEEDBACK_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, []);

  return (
    <main className="flex flex-1 items-center justify-center overflow-hidden">
      {showFeedback ? (
        <div
          role="status"
          aria-live="polite"
          className="flex items-center gap-2 text-sm text-muted-foreground"
        >
          <span className="animate-spin motion-reduce:animate-none" aria-hidden="true">
            <Loader2 className="h-4 w-4" />
          </span>
          <span>{t('installWizardSession.startupDescription')}</span>
        </div>
      ) : null}
    </main>
  );
}

export function InstallWizardSessionGate({ children }: InstallWizardSessionGateProps) {
  const startupPending = useInstallWizardSessionStore(
    (state) => !state.hasConfirmedSnapshot && state.loading && state.syncError === null,
  );

  return startupPending ? <StartupFeedback /> : children;
}
