import { Suspense, useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Outlet } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Header } from '@/components/layout/Header';
import { InstallWizardSessionGate } from '@/components/layout/InstallWizardSessionGate';
import { MutationStatusBar } from '@/components/layout/MutationStatusBar';
import { useEnvironmentRuntimeMonitor } from '@/hooks/useEnvironmentRuntimeMonitor';
import { useInstallWizardSessionMonitor } from '@/hooks/useInstallWizardSessionMonitor';
import { useSkillsDataStore } from '@/stores/skills-data';
import type { SkillLocationRef } from '@/bindings';
import { useEnvironmentStore } from '@/stores/environment';
import { projectWorkspace } from '@/stores/projects';
import {
  selectWorkspaceTransitionActive,
  useWorkspaceContextStore,
} from '@/stores/workspace-context';
import { sameEnvironment } from '@/lib/context';

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
  const environment = useWorkspaceContextStore((state) => state.selectedContext.environment);
  const transitionActive = useWorkspaceContextStore(selectWorkspaceTransitionActive);
  const environmentStatus = useEnvironmentStore((state) => state.environments.find((entry) => (
    sameEnvironment(entry.environment, environment)
  ))?.status);
  useEnvironmentRuntimeMonitor();
  useInstallWizardSessionMonitor();

  useEffect(() => {
    if (transitionActive || environmentStatus !== 'available') return;
    void projectWorkspace.execute({ kind: 'ensureLoaded', environment });
  }, [environment, environmentStatus, transitionActive]);

  useEffect(() => {
    const refreshOnFocus = () => {
      if (transitionActive || environmentStatus !== 'available') return;
      void projectWorkspace.execute({ kind: 'refresh', environment, reason: 'focus' });
    };
    window.addEventListener('focus', refreshOnFocus);
    return () => window.removeEventListener('focus', refreshOnFocus);
  }, [environment, environmentStatus, transitionActive]);

  useEffect(() => {
    const unlisten = listen<{ context: SkillLocationRef; mutatedSkillNames: string[] }>('wizard-result', (event) => {
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
      <InstallWizardSessionGate>
        <main className="flex flex-1 flex-col overflow-hidden">
          <Suspense fallback={<ContentFallback />}>
            <Outlet />
          </Suspense>
        </main>
      </InstallWizardSessionGate>
      <MutationStatusBar />
    </div>
  );
}
