import { useMemo, useCallback, useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useSkillsDataStore } from '@/stores/skills-data';
import { useContextStore } from '@/stores/context';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { DiscoverListPanel } from '@/components/skills/discover/DiscoverListPanel';
import { DiscoverDetailPanel } from '@/components/skills/discover/DiscoverDetailPanel';
import { Compass } from 'lucide-react';
import type { DiscoverSkillSummary, DiscoverTab } from '@/lib/discover/types';
import { getSkillInstallLocations } from '@/lib/discover-utils';
import { ResizablePanelGroup, ResizablePanel, ResizableHandle } from '@/components/ui/resizable';

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
  const globalSkills = useSkillsDataStore((s) => s.globalSkills);
  const allProjectsSkills = useSkillsDataStore((s) => s.allProjectsSkills);
  const fetchAllProjectsSkills = useSkillsDataStore((s) => s.fetchAllProjectsSkills);
  const projects = useContextStore((s) => s.projects);
  const projectsLoaded = useContextStore((s) => s.projectsLoaded);
  const openAddWithPrefill = useSkillDialogStore((s) => s.openAddWithPrefill);

  const [activeTab, setActiveTab] = useState<DiscoverTab>('popular');
  const [selectedSkill, setSelectedSkill] = useState<DiscoverSkillSummary | null>(null);

  useEffect(() => {
    if (projectsLoaded) {
      fetchAllProjectsSkills();
    }
  }, [projectsLoaded, projects, fetchAllProjectsSkills]);

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
    for (const s of globalSkills) addEntry(`${s.source ?? ''}::${s.name}`, 'global');
    for (const [projectPath, skills] of allProjectsSkills) {
      for (const s of skills) addEntry(`${s.source ?? ''}::${s.name}`, projectPath);
    }
    return map;
  }, [globalSkills, allProjectsSkills]);

  const handleInstall = useCallback((skill: DiscoverSkillSummary) => {
    openAddWithPrefill({
      source: skill.source,
      skillName: skill.name,
    });
  }, [openAddWithPrefill]);

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
