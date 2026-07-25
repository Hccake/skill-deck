import { Suspense, useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Outlet } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Header } from '@/components/layout/Header';
import { MutationStatusBar } from '@/components/layout/MutationStatusBar';
import { AgentConfigurationRequestRouter } from '@/components/settings/AgentConfigurationRequestRouter';
import { useEnvironmentRuntimeMonitor } from '@/hooks/useEnvironmentRuntimeMonitor';
import { useSkillsDataStore } from '@/stores/skills-data';
import { useWorkspaceContextStore } from '@/stores/workspace-context';

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

  useEffect(() => {
    const unlisten = listen('wizard-result', () => {
      const committedContext = useWorkspaceContextStore.getState().selectedContext;
      void refreshWorkspace(committedContext);
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [refreshWorkspace]);

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-background text-foreground">
      <Header />
      <AgentConfigurationRequestRouter />
      <main className="flex flex-1 flex-col overflow-hidden">
        <Suspense fallback={<ContentFallback />}>
          <Outlet />
        </Suspense>
      </main>
      <MutationStatusBar />
    </div>
  );
}
