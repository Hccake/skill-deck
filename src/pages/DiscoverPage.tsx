import { useMemo, useCallback, useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useShallow } from 'zustand/react/shallow';
import { useSkillsDataStore, type ContextSkillSnapshot } from '@/stores/skills-data';
import { useWorkspaceContextStore } from '@/stores/workspace-context';
import { contextKey, globalContext } from '@/lib/context';
import { useProjectWorkspace } from '@/hooks/useProjectWorkspace';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { DiscoverListPanel } from '@/components/skills/discover/DiscoverListPanel';
import { DiscoverDetailPanel } from '@/components/skills/discover/DiscoverDetailPanel';
import { Compass } from 'lucide-react';
import type { DiscoverSkillSummary, DiscoverTab } from '@/lib/discover/types';
import { getSkillInstallLocations } from '@/lib/discover-utils';
import { ResizablePanelGroup, ResizablePanel, ResizableHandle } from '@/components/ui/resizable';
import type { SkillLocationRef } from '@/bindings';

const EMPTY_SNAPSHOT: ContextSkillSnapshot = {
  skills: [],
  agents: [],
  pathExists: true,
  loading: false,
  error: null,
  requestId: 0,
};

const DISCOVER_PANEL_LAYOUT = {
  list: {
    defaultSize: '30%',
    minSize: '20%',
    maxSize: '50%',
  },
  detail: {
    defaultSize: '70%',
    minSize: '30%',
  },
} as const;

export function DiscoverPage() {
  const { t } = useTranslation();
  const selectedContext = useWorkspaceContextStore((state) => state.selectedContext);
  const { projects } = useProjectWorkspace(selectedContext.environment);
  const globalSkillContext = useMemo(
    () => globalContext(selectedContext.environment),
    [selectedContext.environment],
  );
  const projectContexts = useMemo<SkillLocationRef[]>(() => projects.map((project) => ({
    environment: selectedContext.environment,
    scope: { scope: 'project', project_id: project.binding.id },
  })), [projects, selectedContext.environment]);
  const projectContextKeys = useMemo(
    () => projectContexts.map(contextKey),
    [projectContexts],
  );
  const globalSnapshot = useSkillsDataStore((state) => (
    state.snapshots[contextKey(globalSkillContext)] ?? EMPTY_SNAPSHOT
  ));
  const projectSnapshots = useSkillsDataStore(useShallow((state) => (
    projectContextKeys.map((key) => state.snapshots[key] ?? EMPTY_SNAPSHOT)
  )));
  const refreshContext = useSkillsDataStore((state) => state.refreshContext);
  const openAddWithPrefill = useSkillDialogStore((s) => s.openAddWithPrefill);

  const [activeTab, setActiveTab] = useState<DiscoverTab>('popular');
  const [selectedSkill, setSelectedSkill] = useState<DiscoverSkillSummary | null>(null);

  useEffect(() => {
    void Promise.all([
      refreshContext(globalSkillContext),
      ...projectContexts.map((context) => refreshContext(context)),
    ]);
  }, [globalSkillContext, projectContexts, refreshContext]);

  const installedSkillLocations = useMemo(() => {
    const map = new Map<string, string[]>();
    const addEntry = (key: string, location: string) => {
      const existing = map.get(key);
      if (existing) {
        if (!existing.includes(location)) existing.push(location);
      } else {
        map.set(key, [location]);
      }
    };
    for (const skill of globalSnapshot.skills) {
      addEntry(`${skill.source ?? ''}::${skill.name}`, 'global');
    }
    for (let index = 0; index < projects.length; index += 1) {
      const projectPath = projects[index].binding.nativePath;
      for (const skill of projectSnapshots[index]?.skills ?? []) {
        addEntry(`${skill.source ?? ''}::${skill.name}`, projectPath);
      }
    }
    return map;
  }, [globalSnapshot.skills, projectSnapshots, projects]);

  const handleInstall = useCallback((skill: DiscoverSkillSummary) => {
    openAddWithPrefill({
      source: skill.source,
      skillName: skill.name,
    }, selectedContext);
  }, [openAddWithPrefill, selectedContext]);

  const installLocations = selectedSkill
    ? getSkillInstallLocations(installedSkillLocations, selectedSkill)
    : [];

  return (
    <div className="flex-1 min-w-0 flex flex-col h-full overflow-hidden bg-background">
      <ResizablePanelGroup
        id="discover-page-layout-fixed"
        orientation="horizontal"
        className="h-full"
      >
        <ResizablePanel
          id="discover-list-fixed"
          defaultSize={DISCOVER_PANEL_LAYOUT.list.defaultSize}
          minSize={DISCOVER_PANEL_LAYOUT.list.minSize}
          maxSize={DISCOVER_PANEL_LAYOUT.list.maxSize}
          className="border-r min-w-0"
        >
          <div className="h-full w-full min-w-0 flex flex-col overflow-hidden">
            <DiscoverListPanel
              installedSkillLocations={installedSkillLocations}
              onSelect={setSelectedSkill}
              selectedDetailUrl={selectedSkill?.detailUrl}
              activeTab={activeTab}
              onTabChange={setActiveTab}
            />
          </div>
        </ResizablePanel>

        <ResizableHandle className="relative w-1.5 bg-border/40 hover:bg-primary/50 transition-colors z-50 after:absolute after:inset-y-0 after:-left-2 after:-right-2 after:cursor-col-resize" />

        <ResizablePanel
          id="discover-detail-fixed"
          defaultSize={DISCOVER_PANEL_LAYOUT.detail.defaultSize}
          minSize={DISCOVER_PANEL_LAYOUT.detail.minSize}
          className="bg-surface relative min-w-0"
        >
          {selectedSkill ? (
            <DiscoverDetailPanel
              skill={selectedSkill}
              installLocations={installLocations}
              onClose={() => setSelectedSkill(null)}
              onInstall={handleInstall}
            />
          ) : (
            <div className="h-full min-w-0 flex flex-col items-center justify-center text-muted-foreground p-8 fading-in">
              <div className="relative mb-6">
                <div className="absolute inset-0 bg-primary/20 blur-xl rounded-full" />
                <Compass className="h-16 w-16 relative text-primary/80 z-10 animate-bounce-subtle" />
              </div>
              <h3 className="text-lg font-heading font-medium tracking-tight text-foreground/80">
                  {t('skills.discover.emptyTitle')}
              </h3>
              <p className="text-sm mt-2 text-center max-w-sm leading-relaxed text-muted-foreground/80">
                  {t('skills.discover.emptyDescription')}
              </p>
            </div>
          )}
        </ResizablePanel>
      </ResizablePanelGroup>
    </div>
  );
}
