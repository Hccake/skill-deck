// src/pages/SkillsPage.tsx
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef } from 'react';
import { useGroupRef } from 'react-resizable-panels';
import { useContextStore } from '@/stores/context';
import { useSkillsDataStore } from '@/stores/skills-data';
import { useSkillDetailStore } from '@/stores/skill-detail';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { createSkillRepairPrefill } from '@/stores/skills-utils';
import { findSkillByIdentity, getSkillIdentityKey } from '@/lib/skills/identity';
import { ContextSidebar, SkillsPanel, SkillDetailPanel } from '@/components/skills';
import { ManageAgentsDialog } from '@/components/skills/ManageAgentsDialog';
import { CopyToProjectDialog } from '@/components/skills/CopyToProjectDialog';
import { checkSkillInProjects } from '@/hooks/useTauriApi';
import { ResizablePanelGroup, ResizablePanel, ResizableHandle } from '@/components/ui/resizable';
import type { InstalledSkill, SkillScope } from '@/bindings';

const SPLIT_VIEW_LAYOUT = {
  'skills-list-panel': 22,
  'skill-detail-panel': 78,
} as const;

const LIST_VIEW_LAYOUT = {
  'skills-list-panel': 100,
} as const;

export function SkillsPage() {
  const selectedContext = useContextStore((s) => s.selectedContext);

  const globalSkills = useSkillsDataStore((s) => s.globalSkills);
  const projectSkills = useSkillsDataStore((s) => s.projectSkills);
  const selectedSkillRef = useSkillDetailStore((s) => s.selectedSkillRef);
  const skillContent = useSkillDetailStore((s) => s.skillContent);
  const loadingContent = useSkillDetailStore((s) => s.loadingContent);
  const deselectSkill = useSkillDetailStore((s) => s.deselectSkill);
  const reloadContent = useSkillDetailStore((s) => s.reloadContent);
  const checkingUpdateScopes = useSkillsDataStore((s) => s.checkingUpdateScopes);
  const forceCheckUpdates = useSkillsDataStore((s) => s.forceCheckUpdates);
  const storeUpdateSkill = useSkillsDataStore((s) => s.updateSkill);
  const updatingSkills = useSkillsDataStore((s) => s.updatingSkills);
  const openDelete = useSkillDialogStore((s) => s.openDelete);
  const openAddWithPrefill = useSkillDialogStore((s) => s.openAddWithPrefill);
  const allAgents = useSkillsDataStore((s) => s.allAgents);
  const openManageAgents = useSkillDialogStore((s) => s.openManageAgents);
  const closeManageAgents = useSkillDialogStore((s) => s.closeManageAgents);
  const saveAgentChanges = useSkillDialogStore((s) => s.saveAgentChanges);
  const manageAgentsSkill = useSkillDialogStore((s) => s.manageAgentsSkill);
  const manageAgentsScope = useSkillDialogStore((s) => s.manageAgentsScope);
  const copySkill = useSkillDialogStore((s) => s.copySkill);
  const openCopyToProject = useSkillDialogStore((s) => s.openCopyToProject);
  const closeCopyToProject = useSkillDialogStore((s) => s.closeCopyToProject);
  const executeCopy = useSkillDialogStore((s) => s.executeCopy);
  const projects = useContextStore((s) => s.projects);
  const layoutRef = useGroupRef();
  const previousContextRef = useRef(selectedContext);

  const selectedSkill = useMemo(
    () => findSkillByIdentity(selectedSkillRef, globalSkills, projectSkills, selectedContext),
    [globalSkills, projectSkills, selectedContext, selectedSkillRef]
  );
  const previousSplitViewRef = useRef(Boolean(selectedSkill));

  const agentDisplayNames = useMemo(
    () => new Map(allAgents.map((a) => [a.id, a.name])),
    [allAgents]
  );
  const selectedSkillUpdateStatus = selectedSkillRef
    ? updatingSkills.get(getSkillIdentityKey(selectedSkillRef))
    : undefined;
  const selectedSkillCheckScope = selectedSkill?.scope === 'project' ? selectedContext : 'global';
  const isCheckingSelectedSkillUpdates = selectedSkill
    ? checkingUpdateScopes.has(selectedSkillCheckScope)
    : false;

  useEffect(() => {
    const contextChanged = previousContextRef.current !== selectedContext;
    previousContextRef.current = selectedContext;

    if (contextChanged || !selectedSkillRef || selectedSkill) return;
    deselectSkill();
  }, [deselectSkill, selectedContext, selectedSkill, selectedSkillRef]);

  const handleDetailDelete = useCallback((skill: InstalledSkill) => {
    if (skill.scope === 'project') {
      openDelete(skill, 'project', selectedContext);
    } else {
      openDelete(skill, 'global');
    }
  }, [openDelete, selectedContext]);

  const handleDetailUpdate = useCallback((name: string, scope: SkillScope) => {
    storeUpdateSkill(name, scope);
  }, [storeUpdateSkill]);

  const handleManageAgents = useCallback((skill: InstalledSkill) => {
    const scope = skill.scope;
    openManageAgents(skill, scope);
  }, [openManageAgents]);

  const handleDetailCheckUpdates = useCallback(() => {
    if (!selectedSkill) return Promise.resolve(false);
    return forceCheckUpdates(selectedSkill.scope);
  }, [forceCheckUpdates, selectedSkill]);

  const handleCopyToProject = useCallback((skill: InstalledSkill) => {
    openCopyToProject(skill);
  }, [openCopyToProject]);

  const handleRepairSource = useCallback((skill: InstalledSkill) => {
    const prefill = createSkillRepairPrefill(
      skill,
      skill.scope,
      skill.scope === 'project' ? selectedContext : undefined
    );
    if (prefill) openAddWithPrefill(prefill);
  }, [openAddWithPrefill, selectedContext]);

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
                  key={selectedSkillRef ? getSkillIdentityKey(selectedSkillRef) : `${selectedSkill.scope}:${selectedSkill.name}`}
                  skill={selectedSkill}
                  content={skillContent}
                  loading={loadingContent}
                  agentDisplayNames={agentDisplayNames}
                  updateStatus={selectedSkillUpdateStatus}
                  isCheckingUpdates={isCheckingSelectedSkillUpdates}
                  projectPath={selectedSkill.scope === 'project' ? selectedContext : undefined}
                  onClose={deselectSkill}
                  onCheckUpdates={handleDetailCheckUpdates}
                  onUpdate={handleDetailUpdate}
                  onDelete={handleDetailDelete}
                  onRetry={reloadContent}
                  onManageAgents={handleManageAgents}
                  onCopyToProject={selectedSkill.scope === 'project' ? handleCopyToProject : undefined}
                  onRepairSource={handleRepairSource}
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
