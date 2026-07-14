// src/pages/SkillsPage.tsx
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef } from 'react';
import { useGroupRef } from 'react-resizable-panels';
import { useWorkspaceContextStore } from '@/stores/workspace-context';
import { contextKey, environmentKey, globalContext } from '@/lib/context';
import { useProjectStore } from '@/stores/projects';
import { useSkillsDataStore, type ContextSkillSnapshot } from '@/stores/skills-data';
import { useSkillDetailStore } from '@/stores/skill-detail';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { findSkillByIdentity, getSkillIdentityKey } from '@/lib/skills/identity';
import { ContextSidebar, SkillsPanel, SkillDetailPanel } from '@/components/skills';
import { ManageAgentsDialog } from '@/components/skills/ManageAgentsDialog';
import { CopyToProjectDialog } from '@/components/skills/CopyToProjectDialog';
import { listSkills } from '@/hooks/useTauriApi';
import { ResizablePanelGroup, ResizablePanel, ResizableHandle } from '@/components/ui/resizable';
import type { ContextRef, InstalledSkill, SkillScope } from '@/bindings';

const EMPTY_SNAPSHOT: ContextSkillSnapshot = {
  skills: [],
  agents: [],
  pathExists: true,
  loading: false,
  error: null,
  requestId: 0,
};
const EMPTY_PROJECTS: ReturnType<typeof useProjectStore.getState>['projectsByEnvironment'][string] = [];

const SPLIT_VIEW_LAYOUT = {
  'skills-list-panel': 22,
  'skill-detail-panel': 78,
} as const;

const LIST_VIEW_LAYOUT = {
  'skills-list-panel': 100,
} as const;

export function SkillsPage() {
  const selectedContext = useWorkspaceContextStore((state) => state.selectedContext);
  const selectedContextKey = contextKey(selectedContext);
  const selectedGlobalContext = globalContext(selectedContext.environment);
  const globalContextKey = contextKey(selectedGlobalContext);
  const projectContextKey = selectedContext.scope.scope === 'project'
    ? selectedContextKey
    : null;
  const projects = useProjectStore((state) => (
    state.projectsByEnvironment[environmentKey(selectedContext.environment)] ?? EMPTY_PROJECTS
  ));
  const selectedScope = selectedContext.scope;
  const selectedProject = selectedScope.scope === 'project'
    ? projects.find((project) => project.binding.id === selectedScope.project_id)
    : null;
  const selectedProjectPath = selectedProject?.binding.nativePath;
  const globalSnapshot = useSkillsDataStore((state) => (
    state.snapshots[globalContextKey] ?? EMPTY_SNAPSHOT
  ));
  const projectSnapshot = useSkillsDataStore((state) => (
    projectContextKey ? state.snapshots[projectContextKey] ?? EMPTY_SNAPSHOT : EMPTY_SNAPSHOT
  ));
  const globalSkills = globalSnapshot.skills;
  const projectSkills = projectSnapshot.skills;
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
  const openRepairSource = useSkillDialogStore((s) => s.openRepairSource);
  const allAgents = selectedContext.scope.scope === 'project'
    ? projectSnapshot.agents
    : globalSnapshot.agents;
  const openManageAgents = useSkillDialogStore((s) => s.openManageAgents);
  const closeManageAgents = useSkillDialogStore((s) => s.closeManageAgents);
  const saveAgentChanges = useSkillDialogStore((s) => s.saveAgentChanges);
  const manageAgentsSkill = useSkillDialogStore((s) => s.manageAgentsSkill);
  const manageAgentsScope = useSkillDialogStore((s) => s.manageAgentsScope);
  const manageAgentDetails = useSkillDialogStore((s) => s.manageAgentDetails);
  const loadingManageAgentDetails = useSkillDialogStore((s) => s.loadingManageAgentDetails);
  const copySkill = useSkillDialogStore((s) => s.copySkill);
  const openCopyToProject = useSkillDialogStore((s) => s.openCopyToProject);
  const closeCopyToProject = useSkillDialogStore((s) => s.closeCopyToProject);
  const executeCopy = useSkillDialogStore((s) => s.executeCopy);
  const copyTargetProjects = projects.map((project) => project.binding.nativePath);
  const checkCopyTargetExistence = useCallback(async (
    skillName: string,
    projectPaths: string[],
  ) => {
    const projectsByPath = new Map(
      projects.map((project) => [project.binding.nativePath, project]),
    );
    return Promise.all(projectPaths.map(async (projectPath) => {
      const project = projectsByPath.get(projectPath);
      if (!project) return { projectPath, hasSkill: false };
      const context: ContextRef = {
        environment: selectedContext.environment,
        scope: { scope: 'project', project_id: project.binding.id },
      };
      try {
        const result = await listSkills(context);
        return {
          projectPath,
          hasSkill: result.skills.some((skill) => skill.name === skillName),
        };
      } catch {
        return { projectPath, hasSkill: false };
      }
    }));
  }, [projects, selectedContext.environment]);
  const layoutRef = useGroupRef();
  const previousContextRef = useRef(selectedContextKey);

  const selectedSkill = useMemo(
    () => findSkillByIdentity(selectedSkillRef, globalSkills, projectSkills, selectedProjectPath),
    [globalSkills, projectSkills, selectedProjectPath, selectedSkillRef]
  );
  const previousSplitViewRef = useRef(Boolean(selectedSkill));

  const agentDisplayNames = useMemo(
    () => new Map(allAgents.map((a) => [a.id, a.name])),
    [allAgents]
  );
  const selectedSkillUpdateStatus = selectedSkillRef
    ? updatingSkills.get(getSkillIdentityKey(selectedSkillRef))
    : undefined;
  const selectedSkillCheckScope = selectedSkill?.scope === 'project'
    ? selectedContextKey
    : globalContextKey;
  const isCheckingSelectedSkillUpdates = selectedSkill
    ? checkingUpdateScopes.has(selectedSkillCheckScope)
    : false;

  useEffect(() => {
    const contextChanged = previousContextRef.current !== selectedContextKey;
    previousContextRef.current = selectedContextKey;

    if (contextChanged || !selectedSkillRef || selectedSkill) return;
    deselectSkill();
  }, [deselectSkill, selectedContextKey, selectedSkill, selectedSkillRef]);

  const handleDetailDelete = useCallback((skill: InstalledSkill) => {
    if (skill.scope === 'project') {
      openDelete(skill, selectedContext, selectedProjectPath);
    } else {
      openDelete(skill, selectedGlobalContext);
    }
  }, [openDelete, selectedContext, selectedGlobalContext, selectedProjectPath]);

  const handleDetailUpdate = useCallback((name: string, scope: SkillScope) => {
    const context = scope === 'project' ? selectedContext : selectedGlobalContext;
    return storeUpdateSkill(context, name);
  }, [selectedContext, selectedGlobalContext, storeUpdateSkill]);

  const handleManageAgents = useCallback((skill: InstalledSkill) => {
    const context = skill.scope === 'project' ? selectedContext : selectedGlobalContext;
    openManageAgents(skill, context, selectedProjectPath);
  }, [openManageAgents, selectedContext, selectedGlobalContext, selectedProjectPath]);

  const handleDetailCheckUpdates = useCallback(() => {
    if (!selectedSkill) return Promise.resolve(false);
    return forceCheckUpdates(
      selectedSkill.scope === 'project' ? selectedContext : selectedGlobalContext,
    );
  }, [forceCheckUpdates, selectedContext, selectedGlobalContext, selectedSkill]);

  const handleCopyToProject = useCallback((skill: InstalledSkill) => {
    openCopyToProject(skill, selectedContext);
  }, [openCopyToProject, selectedContext]);

  const handleRepairSource = useCallback((skill: InstalledSkill) => {
    openRepairSource(
      skill,
      skill.scope === 'project' ? selectedContext : selectedGlobalContext,
      skill.scope === 'project' ? selectedProjectPath : undefined
    );
  }, [openRepairSource, selectedContext, selectedGlobalContext, selectedProjectPath]);

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
                  projectPath={selectedSkill.scope === 'project' ? selectedProjectPath : undefined}
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
      agentDetails={manageAgentDetails}
      loadingAgentDetails={loadingManageAgentDetails}
      onClose={closeManageAgents}
      onSave={saveAgentChanges}
    />

    {/* Copy to Project Dialog */}
    <CopyToProjectDialog
      skill={copySkill}
      currentProjectPath={selectedProjectPath ?? ''}
      projects={copyTargetProjects}
      checkExistence={checkCopyTargetExistence}
      onClose={closeCopyToProject}
      onCopy={executeCopy}
    />
    </>
  );
}
