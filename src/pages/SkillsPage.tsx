// src/pages/SkillsPage.tsx
import { useCallback, useLayoutEffect, useRef } from 'react';
import { useGroupRef } from 'react-resizable-panels';
import { useContextStore } from '@/stores/context';
import { useSkillsStore } from '@/stores/skills';
import { ContextSidebar, SkillsPanel, SkillDetailPanel } from '@/components/skills';
import { ManageAgentsDialog } from '@/components/skills/ManageAgentsDialog';
import { CopyToProjectDialog } from '@/components/skills/CopyToProjectDialog';
import { checkSkillInProjects } from '@/hooks/useTauriApi';
import { ResizablePanelGroup, ResizablePanel, ResizableHandle } from '@/components/ui/resizable';
import type { SkillScope } from '@/bindings';

const SPLIT_VIEW_LAYOUT = {
  'skills-list-panel': 22,
  'skill-detail-panel': 78,
} as const;

const LIST_VIEW_LAYOUT = {
  'skills-list-panel': 100,
} as const;

export function SkillsPage() {
  const selectedContext = useContextStore((s) => s.selectedContext);

  const selectedSkill = useSkillsStore((s) => s.selectedSkill);
  const skillContent = useSkillsStore((s) => s.skillContent);
  const loadingContent = useSkillsStore((s) => s.loadingContent);
  const deselectSkill = useSkillsStore((s) => s.deselectSkill);
  const reloadContent = useSkillsStore((s) => s.reloadContent);
  const storeUpdateSkill = useSkillsStore((s) => s.updateSkill);
  const openDelete = useSkillsStore((s) => s.openDelete);
  const allAgents = useSkillsStore((s) => s.allAgents);
  const openManageAgents = useSkillsStore((s) => s.openManageAgents);
  const closeManageAgents = useSkillsStore((s) => s.closeManageAgents);
  const saveAgentChanges = useSkillsStore((s) => s.saveAgentChanges);
  const manageAgentsSkill = useSkillsStore((s) => s.manageAgentsSkill);
  const manageAgentsScope = useSkillsStore((s) => s.manageAgentsScope);
  const copySkill = useSkillsStore((s) => s.copySkill);
  const openCopyToProject = useSkillsStore((s) => s.openCopyToProject);
  const closeCopyToProject = useSkillsStore((s) => s.closeCopyToProject);
  const executeCopy = useSkillsStore((s) => s.executeCopy);
  const projects = useContextStore((s) => s.projects);
  const layoutRef = useGroupRef();
  const previousSplitViewRef = useRef(Boolean(selectedSkill));

  const agentDisplayNames = new Map(allAgents.map((a) => [a.id, a.name]));

  const handleDetailDelete = useCallback((skill: typeof selectedSkill & {}) => {
    if (skill.scope === 'project') {
      openDelete(skill, 'project', selectedContext);
    } else {
      openDelete(skill, 'global');
    }
  }, [openDelete, selectedContext]);

  const handleDetailUpdate = useCallback((name: string, scope: SkillScope) => {
    storeUpdateSkill(name, scope);
  }, [storeUpdateSkill]);

  const handleManageAgents = useCallback((skill: typeof selectedSkill & {}) => {
    const scope = skill.scope;
    openManageAgents(skill, scope);
  }, [openManageAgents]);

  const handleCopyToProject = useCallback((skill: typeof selectedSkill & {}) => {
    openCopyToProject(skill);
  }, [openCopyToProject]);

  useLayoutEffect(() => {
    const hasDetail = Boolean(selectedSkill);
    const hadDetail = previousSplitViewRef.current;

    if (hasDetail === hadDetail) return;

    const expectedPanelCount = hasDetail ? 2 : 1;
    const nextLayout = hasDetail ? SPLIT_VIEW_LAYOUT : LIST_VIEW_LAYOUT;
    let cancelled = false;

    const applyLayoutWhenReady = (attempt = 0) => {
      if (cancelled) return;

      const group = layoutRef.current;
      if (!group) return;

      if (Object.keys(group.getLayout()).length === expectedPanelCount) {
        group.setLayout(nextLayout);
        previousSplitViewRef.current = hasDetail;
        return;
      }

      if (attempt < 10) {
        queueMicrotask(() => applyLayoutWhenReady(attempt + 1));
      }
    };

    applyLayoutWhenReady();

    return () => {
      cancelled = true;
    };
  }, [selectedSkill, layoutRef]);

  return (
    <>
    <div className="skills-page-shell flex h-full min-w-0">
      {/* Left Sidebar: Context */}
      <ContextSidebar />

      {/* Main content area with height constraint */}
      <div className="flex-1 min-w-0 overflow-hidden">
        <ResizablePanelGroup
          id="skills-page-layout"
          orientation="horizontal"
          className="h-full"
          groupRef={layoutRef}
        >
          {/* Skills list panel */}
          <ResizablePanel
            id="skills-list-panel"
            defaultSize={selectedSkill ? '22%' : '100%'}
            minSize={selectedSkill ? '12%' : '100%'}
            maxSize={selectedSkill ? '85%' : '100%'}
          >
            <SkillsPanel compact={!!selectedSkill} />
          </ResizablePanel>

          {/* Detail panel — only when a skill is selected */}
          {selectedSkill && (
            <>
              <ResizableHandle className="bg-transparent" />
              <ResizablePanel
                id="skill-detail-panel"
                defaultSize="78%"
                minSize="15%"
                className="bg-surface relative"
              >
                <SkillDetailPanel
                  skill={selectedSkill}
                  content={skillContent}
                  loading={loadingContent}
                  agentDisplayNames={agentDisplayNames}
                  onClose={deselectSkill}
                  onUpdate={handleDetailUpdate}
                  onDelete={handleDetailDelete}
                  onRetry={reloadContent}
                  onManageAgents={handleManageAgents}
                  onCopyToProject={selectedSkill.scope === 'project' ? handleCopyToProject : undefined}
                />
              </ResizablePanel>
            </>
          )}
        </ResizablePanelGroup>
      </div>
    </div>

    {/* Manage Agents Dialog */}
    <ManageAgentsDialog
      skill={manageAgentsSkill}
      scope={manageAgentsScope}
      allAgents={allAgents}
      onClose={closeManageAgents}
      onSave={saveAgentChanges}
    />

    {/* Copy to Project Dialog */}
    <CopyToProjectDialog
      skill={copySkill}
      currentProjectPath={selectedContext}
      projects={projects}
      checkExistence={checkSkillInProjects}
      onClose={closeCopyToProject}
      onCopy={executeCopy}
    />
    </>
  );
}
