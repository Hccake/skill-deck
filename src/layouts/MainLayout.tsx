import { Suspense, useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Outlet } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Header } from '@/components/layout/Header';
import { InstallWizardReadOnlyBar } from '@/components/layout/InstallWizardReadOnlyBar';
import { MutationStatusBar } from '@/components/layout/MutationStatusBar';
import { useEnvironmentRuntimeMonitor } from '@/hooks/useEnvironmentRuntimeMonitor';
import { useInstallWizardSessionMonitor } from '@/hooks/useInstallWizardSessionMonitor';
import { useSkillsDataStore } from '@/stores/skills-data';
import type { ContextRef } from '@/bindings';

function ContentFallback() {
  const { t } = useTranslation();
  return (
    <div role="status" aria-live="polite" className="flex h-full items-center justify-center text-sm text-muted-foreground">
      {t('common.loading')}
    </div>
  );
}

export default function MainLayout() {
  const refreshWorkspace = useSkillsDataStore((state) => state.refreshWorkspace);
  useEnvironmentRuntimeMonitor();
  useInstallWizardSessionMonitor();

  useEffect(() => {
    const unlisten = listen<{ context: ContextRef; mutatedSkillNames: string[] }>('wizard-result', (event) => {
      void refreshWorkspace(event.payload.context, {
        origin: 'selfMutation',
        mutatedSkillNames: event.payload.mutatedSkillNames,
      });
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [refreshWorkspace]);

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-background text-foreground">
      <Header />
      <InstallWizardReadOnlyBar />
      <main className="flex flex-1 flex-col overflow-hidden">
        <Suspense fallback={<ContentFallback />}>
          <Outlet />
        </Suspense>
      </main>
      <MutationStatusBar />
    </div>
  );
}
