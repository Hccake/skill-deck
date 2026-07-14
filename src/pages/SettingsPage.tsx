import { useEffect, useRef } from 'react';
import { useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Settings2, SlidersHorizontal, GitBranch, FolderOpen, Info } from 'lucide-react';
import { cn } from '@/lib/utils';
import { useWorkspaceContextStore } from '@/stores/workspace-context';
import { useProjectStore } from '@/stores/projects';
import { environmentKey } from '@/lib/context';
import { useSettingsStore, type AgentDefaultsSnapshot } from '@/stores/settings';
import { AboutTab } from '@/components/settings/AboutTab';
import { GeneralTab } from '@/components/settings/GeneralTab';
import { GitSettingsPage } from '@/components/settings/GitSettingsPage';
import { InstallPreferencesPage } from '@/components/settings/InstallPreferencesPage';
import { ProjectsTab } from '@/components/settings/ProjectsTab';

type SettingsSectionId = 'general' | 'install-preferences' | 'git' | 'projects' | 'about';

const SETTINGS_SECTIONS: Array<{
  id: SettingsSectionId;
  icon: typeof Settings2;
  titleKey: string;
}> = [
  {
    id: 'general',
    icon: Settings2,
    titleKey: 'settings.nav.general',
  },
  {
    id: 'install-preferences',
    icon: SlidersHorizontal,
    titleKey: 'settings.nav.installPreferences',
  },
  {
    id: 'git',
    icon: GitBranch,
    titleKey: 'settings.nav.git',
  },
  {
    id: 'projects',
    icon: FolderOpen,
    titleKey: 'settings.nav.projects',
  },
];

const DEFAULT_SECTION: SettingsSectionId = 'general';
const VALID_SECTION_IDS: SettingsSectionId[] = ['general', 'install-preferences', 'git', 'projects', 'about'];

const EMPTY_AGENT_DEFAULTS_SNAPSHOT: AgentDefaultsSnapshot = {
  agents: [],
  defaults: { global: [], project: [] },
  loadState: 'idle',
  loadRequestId: 0,
  saveRequestId: 0,
  saving: false,
  error: null,
};

function isSettingsSection(value: string | null): value is SettingsSectionId {
  return !!value && VALID_SECTION_IDS.includes(value as SettingsSectionId);
}

export function SettingsPage() {
  const { t } = useTranslation();
  const [searchParams, setSearchParams] = useSearchParams();
  const selectedContext = useWorkspaceContextStore((state) => state.selectedContext);
  const selectedEnvironmentKey = environmentKey(selectedContext.environment);
  const projectLoadState = useProjectStore((state) => state.loadStateByEnvironment[selectedEnvironmentKey]);
  const refreshProjects = useProjectStore((state) => state.refresh);
  const agentDefaultsSnapshot = useSettingsStore(
    (state) => state.agentDefaultsByEnvironment[selectedEnvironmentKey],
  );
  const loadAgentDefaults = useSettingsStore((state) => state.loadAgentDefaults);
  const lastAgentDefaultsEnvironment = useRef<string | null>(null);

  useEffect(() => {
    if (projectLoadState !== 'ready' && projectLoadState !== 'loading') {
      void refreshProjects(selectedContext.environment);
    }
  }, [projectLoadState, refreshProjects, selectedContext.environment]);

  useEffect(() => {
    if (lastAgentDefaultsEnvironment.current === selectedEnvironmentKey) return;
    lastAgentDefaultsEnvironment.current = selectedEnvironmentKey;
    if (!agentDefaultsSnapshot
      || agentDefaultsSnapshot.loadState === 'idle'
      || agentDefaultsSnapshot.loadState === 'error') {
      void loadAgentDefaults(selectedContext.environment);
    }
  }, [
    agentDefaultsSnapshot,
    loadAgentDefaults,
    selectedContext.environment,
    selectedEnvironmentKey,
  ]);

  const sectionParam = searchParams.get('section');
  const activeSection: SettingsSectionId = isSettingsSection(sectionParam)
    ? sectionParam
    : DEFAULT_SECTION;

  const setSection = (sectionId: SettingsSectionId) => {
    const nextParams = new URLSearchParams(searchParams);
    if (sectionId === DEFAULT_SECTION) {
      nextParams.delete('section');
    } else {
      nextParams.set('section', sectionId);
    }
    setSearchParams(nextParams, { replace: false });
  };

  const renderSection = () => {
    switch (activeSection) {
      case 'install-preferences':
        return (
          <InstallPreferencesPage
            environment={selectedContext.environment}
            snapshot={agentDefaultsSnapshot ?? EMPTY_AGENT_DEFAULTS_SNAPSHOT}
          />
        );
      case 'git':
        return <GitSettingsPage />;
      case 'projects':
        return <ProjectsTab />;
      case 'about':
        return <AboutTab />;
      case 'general':
      default:
        return <GeneralTab />;
    }
  };

  return (
    <div className="flex h-full min-h-0 overflow-hidden bg-muted/10">
      <aside className="flex w-[64px] lg:w-56 shrink-0 border-r border-border/60 bg-background/90 backdrop-blur transition-all duration-300">
        <div className="flex h-full w-full flex-col px-2.5 lg:px-3 py-5">
          <div className="px-1 lg:px-2 pb-5 flex items-center justify-center lg:justify-start">
            <h1 className="hidden lg:block text-lg font-semibold tracking-tight text-foreground">
              {t('settings.title')}
            </h1>
            <Settings2 className="lg:hidden h-5 w-5 text-muted-foreground" />
          </div>

          <nav className="space-y-0.5">
            {SETTINGS_SECTIONS.map((section) => {
              const Icon = section.icon;
              const selected = section.id === activeSection;

              return (
                <button
                  key={section.id}
                  type="button"
                  title={t(section.titleKey)}
                  onClick={() => setSection(section.id)}
                  className={cn(
                    'group relative flex h-10 w-full cursor-pointer items-center justify-center lg:justify-start gap-2.5 rounded-lg lg:px-3 text-left text-sm transition-colors',
                    selected
                      ? 'bg-muted/50 font-medium text-foreground shadow-sm'
                      : 'text-muted-foreground hover:bg-muted/30 hover:text-foreground'
                  )}
                >
                  {selected ? (
                    <span className="absolute inset-y-2 left-0 w-0.5 rounded-full bg-primary" />
                  ) : null}
                  <Icon
                    className={cn(
                      'h-4 w-4 shrink-0',
                      selected ? 'text-primary' : 'text-muted-foreground group-hover:text-foreground'
                    )}
                  />
                  <span className="hidden lg:inline truncate">
                    {t(section.titleKey)}
                  </span>
                </button>
              );
            })}
          </nav>

          <button
            type="button"
            title={t('settings.nav.about')}
            onClick={() => setSection('about')}
            className={cn(
              'group relative mt-auto flex h-10 w-full cursor-pointer items-center justify-center lg:justify-start gap-2.5 rounded-lg lg:px-3 text-left text-sm transition-colors',
              activeSection === 'about'
                ? 'bg-muted/50 font-medium text-foreground shadow-sm'
                : 'text-muted-foreground hover:bg-muted/30 hover:text-foreground'
            )}
          >
            {activeSection === 'about' ? (
              <span className="absolute inset-y-2 left-0 w-0.5 rounded-full bg-primary" />
            ) : null}
            <Info
              className={cn(
                'h-4 w-4 shrink-0',
                activeSection === 'about' ? 'text-primary' : 'text-muted-foreground group-hover:text-foreground'
              )}
            />
            <span className="hidden lg:inline truncate">{t('settings.nav.about')}</span>
          </button>
        </div>
      </aside>

      <main className="flex-1 overflow-auto">
        <div className="mx-auto flex min-h-full max-w-5xl flex-col gap-4 px-4 py-5 sm:px-6 lg:px-10">


          <div className="min-h-0 flex-1">
            <div key={activeSection} className="animate-in fade-in duration-300 slide-in-from-bottom-1.5 h-full">
              {renderSection()}
            </div>
          </div>
        </div>
      </main>
    </div>
  );
}
