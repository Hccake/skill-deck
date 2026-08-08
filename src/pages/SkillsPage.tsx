// src/pages/SkillsPage.tsx
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef } from 'react';
import { useGroupRef } from 'react-resizable-panels';
import { useWorkspaceContextStore } from '@/stores/workspace-context';
import { contextKey, globalContext } from '@/lib/context';
import { useProjectWorkspace } from '@/hooks/useProjectWorkspace';
import {
  sourceDiagnosticsForEnvironment,
  useSkillsDataStore,
  type ContextSkillSnapshot,
} from '@/stores/skills-data';
import { useSkillDetailStore } from '@/stores/skill-detail';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { findSkillByIdentity, getSkillIdentityKey } from '@/lib/skills/identity';
import { agentDisplayName, agentId } from '@/lib/agents';
import { ContextSidebar, SkillsPanel, SkillDetailPanel } from '@/components/skills';
import { ManageAgentsDialogContainer } from '@/components/skills/ManageAgentsDialogContainer';
import { CopyToProjectDialogContainer } from '@/components/skills/CopyToProjectDialogContainer';
import { UpdatePlanDialogContainer } from '@/components/skills/UpdatePlanDialogContainer';
import { useSkillUpdateWorkflow } from '@/workflows/skill-update';
import { openManageAgentChanges } from '@/workflows/skill-manage-agents';
import { openSkillRemoval } from '@/workflows/skill-remove';
import { ResizablePanelGroup, ResizablePanel, ResizableHandle } from '@/components/ui/resizable';
import type { InstalledSkill, InstalledSkillLocation } from '@/bindings';

const EMPTY_SNAPSHOT: ContextSkillSnapshot = {
  skills: [],
  agents: [],
  pathExists: true,
  loading: false,
  error: null,
  requestId: 0,
};
const EMPTY_SKILL_NAMES: string[] = [];
const EMPTY_SCOPE_KEYS = new Set<string>();

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
  const { projects } = useProjectWorkspace(selectedContext.environment);
  const selectedScope = selectedContext.scope;
  const selectedProject = selectedScope.scope === 'project'
    ? projects.find((project) => project.binding.id === selectedScope.project_id)
    : null;
  const selectedProjectPath = selectedProject?.binding.nativePath;
  const snapshots = useSkillsDataStore((state) => state.snapshots);
  const globalSnapshot = snapshots[globalContextKey] ?? EMPTY_SNAPSHOT;
  const projectSnapshot = projectContextKey
    ? snapshots[projectContextKey] ?? EMPTY_SNAPSHOT
    : EMPTY_SNAPSHOT;
  const environmentSourceDiagnostics = useMemo(
    () => sourceDiagnosticsForEnvironment(snapshots, selectedContext.environment),
    [selectedContext.environment, snapshots],
  );
  const globalSkills = globalSnapshot.skills;
  const projectSkills = projectSnapshot.skills;
  const selectedSkillRef = useSkillDetailStore((s) => s.selectedSkillRef);
  const skillContent = useSkillDetailStore((s) => s.skillContent);
  const loadingContent = useSkillDetailStore((s) => s.loadingContent);
  const deselectSkill = useSkillDetailStore((s) => s.deselectSkill);
  const reloadContent = useSkillDetailStore((s) => s.reloadContent);
  const forceUpdateScopes = useSkillsDataStore((s) => (
    s.forceUpdateScopes ?? s.checkingUpdateScopes ?? EMPTY_SCOPE_KEYS
  ));
  const forceCheckUpdates = useSkillsDataStore((s) => s.forceCheckUpdates);
  const updatingContext = useSkillUpdateWorkflow((s) => (
    s.phase === 'executing' ? s.context : null
  ));
  const updatingSkillNames = useSkillUpdateWorkflow((s) => (
    s.phase === 'executing' ? s.skillNames : EMPTY_SKILL_NAMES
  ));
  const openUpdate = useSkillUpdateWorkflow((s) => s.open);
  const openRepairSource = useSkillDialogStore((s) => s.openRepairSource);
  const allAgents = selectedContext.scope.scope === 'project'
    ? projectSnapshot.agents
    : globalSnapshot.agents;
  const openCopyToProject = useSkillDialogStore((s) => s.openCopyToProject);
  const layoutRef = useGroupRef();
  const previousContextRef = useRef(selectedContextKey);

  const selectedSkill = useMemo(
    () => findSkillByIdentity(selectedSkillRef, globalSkills, projectSkills, selectedProjectPath),
    [globalSkills, projectSkills, selectedProjectPath, selectedSkillRef]
  );
  const previousSplitViewRef = useRef(Boolean(selectedSkill));

  const agentDisplayNames = useMemo(
    () => new Map(allAgents.map((agent) => [agentId(agent), agentDisplayName(agent)])),
    [allAgents]
  );
  const selectedSkillUpdateStatus = selectedSkillRef
    && updatingContext
    && contextKey(updatingContext) === (
      selectedSkillRef.scope === 'project' ? selectedContextKey : globalContextKey
    )
    && updatingSkillNames.includes(selectedSkillRef.name)
    ? 'updating'
    : undefined;
  const selectedSkillCheckScope = selectedSkill?.scope === 'project'
    ? selectedContextKey
    : globalContextKey;
  const isCheckingSelectedSkillUpdates = selectedSkill
    ? forceUpdateScopes.has(selectedSkillCheckScope)
    : false;

  useEffect(() => {
    const contextChanged = previousContextRef.current !== selectedContextKey;
    previousContextRef.current = selectedContextKey;

    if (contextChanged || !selectedSkillRef || selectedSkill) return;
    deselectSkill();
  }, [deselectSkill, selectedContextKey, selectedSkill, selectedSkillRef]);

  const handleDetailDelete = useCallback((skill: InstalledSkill) => {
    if (skill.scope === 'project') {
      void openSkillRemoval(skill, selectedContext, selectedProjectPath);
    } else {
      void openSkillRemoval(skill, selectedGlobalContext);
    }
  }, [selectedContext, selectedGlobalContext, selectedProjectPath]);

  const handleDetailUpdate = useCallback(async (name: string, scope: InstalledSkillLocation) => {
    const context = scope === 'project' ? selectedContext : selectedGlobalContext;
    await openUpdate(context, [name], false);
  }, [openUpdate, selectedContext, selectedGlobalContext]);

  const handleManageAgents = useCallback((skill: InstalledSkill) => {
    const context = skill.scope === 'project' ? selectedContext : selectedGlobalContext;
    void openManageAgentChanges(skill, context, selectedProjectPath);
  }, [selectedContext, selectedGlobalContext, selectedProjectPath]);

  const handleDetailCheckUpdates = useCallback(() => {
    if (!selectedSkill) return Promise.resolve(null);
    return forceCheckUpdates(
      selectedSkill.scope === 'project' ? selectedContext : selectedGlobalContext,
      {
        kind: 'skills',
        skills: [{
          context: selectedSkill.scope === 'project' ? selectedContext : selectedGlobalContext,
          skillName: selectedSkill.name,
        }],
      },
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
                  sourceDiagnostics={environmentSourceDiagnostics}
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

    <ManageAgentsDialogContainer />
    <CopyToProjectDialogContainer />
    <UpdatePlanDialogContainer />
    </>
  );
}
