// src/pages/SkillsPage.tsx
import { useCallback, useEffect, useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { useWorkspaceContextStore } from '@/stores/workspace-context';
import { contextKey, globalContext } from '@/lib/context';
import { useProjectWorkspace } from '@/hooks/useProjectWorkspace';
import {
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
import { openSkillRemoval } from '@/workflows/skill-remove';
import { SkillsWorkspace } from '@/components/skills/SkillsWorkspace';
import { OverflowTooltip } from '@/components/ui/overflow-tooltip';
import { projectDisplayName } from '@/lib/projects/presentation';
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

export function SkillsPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
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
  const contextLabel = selectedScope.scope === 'project'
    ? selectedProject ? projectDisplayName(selectedProject) : t('context.projects')
    : t('context.global');
  const selectedProjectPath = selectedProject?.binding.nativePath;
  const snapshots = useSkillsDataStore((state) => state.snapshots);
  const globalSnapshot = snapshots[globalContextKey] ?? EMPTY_SNAPSHOT;
  const projectSnapshot = projectContextKey
    ? snapshots[projectContextKey] ?? EMPTY_SNAPSHOT
    : EMPTY_SNAPSHOT;
  const globalSkills = globalSnapshot.skills;
  const projectSkills = projectSnapshot.skills;
  const selectedSkillRef = useSkillDetailStore((s) => s.selectedSkillRef);
  const skillContent = useSkillDetailStore((s) => s.skillContent);
  const loadingContent = useSkillDetailStore((s) => s.loadingContent);
  const contentError = useSkillDetailStore((s) => s.contentError);
  const deselectSkill = useSkillDetailStore((s) => s.deselectSkill);
  const detailOpen = useSkillDetailStore((s) => s.detailOpen);
  const closeDetail = useSkillDetailStore((s) => s.closeDetail);
  const reloadContent = useSkillDetailStore((s) => s.reloadContent);
  const forceUpdateScopes = useSkillsDataStore((s) => s.forceUpdateScopes);
  const forceCheckUpdates = useSkillsDataStore((s) => s.forceCheckUpdates);
  const updatingContext = useSkillUpdateWorkflow((s) => (
    s.phase === 'executing' ? s.context : null
  ));
  const updatingSkillNames = useSkillUpdateWorkflow((s) => (
    s.phase === 'executing' ? s.skillNames : EMPTY_SKILL_NAMES
  ));
  const openUpdate = useSkillUpdateWorkflow((s) => s.open);
  const openManageAgents = useSkillDialogStore((s) => s.openManageAgents);
  const allAgents = selectedContext.scope.scope === 'project'
    ? projectSnapshot.agents
    : globalSnapshot.agents;
  const openCopyToProject = useSkillDialogStore((s) => s.openCopyToProject);
  const previousContextRef = useRef(selectedContextKey);

  const selectedSkill = useMemo(
    () => findSkillByIdentity(selectedSkillRef, globalSkills, projectSkills, selectedProjectPath),
    [globalSkills, projectSkills, selectedProjectPath, selectedSkillRef]
  );

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
    openManageAgents(skill, context);
  }, [openManageAgents, selectedContext, selectedGlobalContext]);

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

  return (
    <>
      <SkillsWorkspace
        contextKey={selectedContextKey}
        detailOpen={detailOpen}
        onCloseDetail={closeDetail}
        sidebar={<ContextSidebar />}
        detail={selectedSkill ? (
          <SkillDetailPanel
            key={`${selectedSkill.scope === 'project' ? selectedContextKey : globalContextKey}:${selectedSkillRef ? getSkillIdentityKey(selectedSkillRef) : selectedSkill.name}`}
            skill={selectedSkill}
            content={skillContent}
            loading={loadingContent}
            contentError={contentError}
            agentDisplayNames={agentDisplayNames}
            updateStatus={selectedSkillUpdateStatus}
            isCheckingUpdates={isCheckingSelectedSkillUpdates}
            context={selectedSkill.scope === 'project' ? selectedContext : selectedGlobalContext}
            pathBase={(selectedSkill.scope === 'project' ? projectSnapshot : globalSnapshot).pathBase}
            onClose={closeDetail}
            onCheckUpdates={handleDetailCheckUpdates}
            onUpdate={handleDetailUpdate}
            onDelete={handleDetailDelete}
            onRetry={reloadContent}
            onManageAgents={handleManageAgents}
            onCopyToProject={selectedSkill.scope === 'project' ? handleCopyToProject : undefined}
            onConfigureGitCredentials={() => navigate('/settings?section=git')}
            onOpenLibraryVersion={(version) => navigate(`/libraries?${new URLSearchParams({ library: version.libraryId, skill: version.skillName })}`)}
          />
        ) : null}
      >
        {(navigation) => <>
          {navigation ? (
            <div className="flex shrink-0 items-center gap-2 border-b bg-panel px-3 py-1">
              {navigation}
              <OverflowTooltip content={contextLabel}><span tabIndex={0} className="min-w-0 truncate text-sm font-medium">{contextLabel}</span></OverflowTooltip>
            </div>
          ) : null}
          <div className="min-h-0 flex-1">
            <SkillsPanel compact={detailOpen} />
          </div>
        </>}
      </SkillsWorkspace>

      <ManageAgentsDialogContainer />
      <CopyToProjectDialogContainer />
      <UpdatePlanDialogContainer />
    </>
  );
}
