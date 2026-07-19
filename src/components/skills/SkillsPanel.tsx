// src/components/skills/SkillsPanel.tsx
import { useState, useEffect, useMemo, useCallback, useDeferredValue, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { RefreshCw } from 'lucide-react';
import { useWorkspaceContextStore } from '@/stores/workspace-context';
import { useProjectStore } from '@/stores/projects';
import { useSkillsDataStore, type ContextSkillSnapshot } from '@/stores/skills-data';
import { useSkillDetailStore } from '@/stores/skill-detail';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { SkillsToolbar } from './SkillsToolbar';
import { SkillsSection } from './SkillsSection';
import { CompactSkillList } from './CompactSkillList';
import { CrossStorageWarningBanner } from './CrossStorageWarningBanner';
import { DeleteSkillDialog } from './DeleteSkillDialog';
import { RepairSourceDialog } from './RepairSourceDialog';
import { GlobalEmptyState, ProjectEmptyState } from './EmptyStates';
import { Skeleton } from '@/components/ui/skeleton';
import { Button } from '@/components/ui/button';
import { contextKey, environmentKey, globalContext } from '@/lib/context';
import { agentDisplayName, agentId } from '@/lib/agents';
import { formatAppError } from '@/utils/format-app-error';
import { openSkillRemoval } from '@/workflows/skill-remove';
import { openManageAgentChanges } from '@/workflows/skill-manage-agents';
import { useSkillUpdateWorkflow } from '@/workflows/skill-update';
import { getSkillIdentityKey } from '@/lib/skills/identity';
import type { SkillUpdateDisplayStatus } from '@/stores/skills-utils';
import type { InstalledSkill } from '@/bindings';

const EMPTY_SNAPSHOT: ContextSkillSnapshot = {
  skills: [],
  agents: [],
  pathExists: true,
  loading: false,
  error: null,
  requestId: 0,
};
const EMPTY_PROJECTS: ReturnType<typeof useProjectStore.getState>['projectsByEnvironment'][string] = [];

/** 按搜索关键词 + agent 筛选过滤 skills — 单次遍历 (js-combine-iterations) */
function filterSkills<T extends InstalledSkill>(skills: T[], searchQuery: string, agentFilter: string): T[] {
  if (!searchQuery && agentFilter === 'all') return skills;
  const query = searchQuery ? searchQuery.toLowerCase() : '';
  return skills.filter((s) => {
    if (query && !s.name.toLowerCase().includes(query) && !s.description.toLowerCase().includes(query)) {
      return false;
    }
    if (agentFilter !== 'all' && !s.agents.includes(agentFilter)) {
      return false;
    }
    return true;
  });
}

interface SkillsPanelProps {
  /** 紧凑模式 — 选中 skill 后由 SkillsPage 传入 */
  compact: boolean;
}

export function SkillsPanel({ compact }: SkillsPanelProps) {
  const { t } = useTranslation();
  const selectedContext = useWorkspaceContextStore((state) => state.selectedContext);
  const selectedContextKey = contextKey(selectedContext);
  const selectedGlobalContext = globalContext(selectedContext.environment);
  const globalContextKey = contextKey(selectedGlobalContext);
  const isProjectSelected = selectedContext.scope.scope === 'project';
  const projectContextKey = isProjectSelected ? selectedContextKey : null;
  const projects = useProjectStore((state) => (
    state.projectsByEnvironment[environmentKey(selectedContext.environment)] ?? EMPTY_PROJECTS
  ));
  const selectedScope = selectedContext.scope;
  const selectedProject = selectedScope.scope === 'project'
    ? projects.find((project) => project.binding.id === selectedScope.project_id)
    : null;
  const projectPath = selectedProject?.binding.nativePath;

  // ① Store — 细粒度 selector 订阅
  const globalSnapshot = useSkillsDataStore((state) => (
    state.snapshots[globalContextKey] ?? EMPTY_SNAPSHOT
  ));
  const projectSnapshot = useSkillsDataStore((state) => (
    projectContextKey ? state.snapshots[projectContextKey] ?? EMPTY_SNAPSHOT : EMPTY_SNAPSHOT
  ));
  const globalSkills = globalSnapshot.skills;
  const projectSkills = isProjectSelected ? projectSnapshot.skills : EMPTY_SNAPSHOT.skills;
  const projectPathExists = projectSnapshot.pathExists;
  const allAgents = isProjectSelected ? projectSnapshot.agents : globalSnapshot.agents;
  const loading = (globalSnapshot.loading && globalSkills.length === 0)
    || (isProjectSelected && projectSnapshot.loading && projectSkills.length === 0);
  const error = projectSnapshot.error ?? globalSnapshot.error;
  const isSyncing = useSkillsDataStore((s) => s.isSyncing);
  const isCheckingGlobal = useSkillsDataStore((s) => s.checkingUpdateScopes.has(globalContextKey));
  const isCheckingProject = useSkillsDataStore((s) => (
    projectContextKey ? s.checkingUpdateScopes.has(projectContextKey) : false
  ));
  const syncUpdates = useSkillsDataStore((s) => s.syncUpdates);
  const forceCheckUpdates = useSkillsDataStore((s) => s.forceCheckUpdates);
  const updatePhase = useSkillUpdateWorkflow((s) => s.phase);
  const updateContext = useSkillUpdateWorkflow((s) => s.context);
  const updateSkillNames = useSkillUpdateWorkflow((s) => s.skillNames);
  const openUpdate = useSkillUpdateWorkflow((s) => s.open);
  const refreshWorkspace = useSkillsDataStore((s) => s.refreshWorkspace);
  const syncSkills = useSkillsDataStore((s) => s.syncSkills);
  const auditCache = useSkillsDataStore((s) => s.auditCache);
  const fetchAuditForSkills = useSkillsDataStore((s) => s.fetchAuditForSkills);
  const selectSkill = useSkillDetailStore((s) => s.selectSkill);
  const deselectSkill = useSkillDetailStore((s) => s.deselectSkill);
  const selectedSkillRef = useSkillDetailStore((s) => s.selectedSkillRef);
  const openAdd = useSkillDialogStore((s) => s.openAdd);
  const openRepairSource = useSkillDialogStore((s) => s.openRepairSource);
  const openCopyToProject = useSkillDialogStore((s) => s.openCopyToProject);

  // ② UI 状态 — 仅 2 个 useState
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedAgentFilter, setSelectedAgentFilter] = useState('all');

  // 搜索优化：列表过滤作为低优先级更新 (rerender-transitions)
  const deferredQuery = useDeferredValue(searchQuery);

  // A single timer owns automatic checks. Focus reuses this path instead of adding listeners per section.
  useEffect(() => {
    let ignore = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const schedule = () => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => { if (!ignore) void syncUpdates(selectedContext); }, 500);
    };
    schedule();
    void refreshWorkspace(selectedContext);
    window.addEventListener('focus', schedule);
    return () => { ignore = true; if (timer) clearTimeout(timer); window.removeEventListener('focus', schedule); };
  }, [selectedContext, selectedContextKey, refreshWorkspace, syncUpdates]);

  // ③a 仅在 context 真正切换时关闭详情面板
  const previousContextRef = useRef(selectedContextKey);
  useEffect(() => {
    if (previousContextRef.current !== selectedContextKey) {
      deselectSkill();
      previousContextRef.current = selectedContextKey;
    }
  }, [selectedContextKey, deselectSkill]);

  // Esc 键关闭详情面板
  useEffect(() => {
    if (!selectedSkillRef) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') deselectSkill();
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [selectedSkillRef, deselectSkill]);

  // ③b 审计数据 — skills 变化后获取（仅对有 source 的 skills 请求）
  useEffect(() => {
    const allSkills = [...globalSkills, ...projectSkills];
    const skillsWithSource = allSkills.filter((s) => s.source);
    if (skillsWithSource.length > 0) {
      fetchAuditForSkills(skillsWithSource);
    }
  }, [globalSkills, projectSkills, fetchAuditForSkills]);

  const filterableAgents = useMemo(() => {
    const agentIds = new Set<string>();
    const allSkills = isProjectSelected ? [...globalSkills, ...projectSkills] : globalSkills;
    for (const s of allSkills) {
      for (const id of s.agents) agentIds.add(id);
    }
    return allAgents
      .filter((agent) => agentIds.has(agentId(agent)))
      .sort((left, right) => agentDisplayName(left).localeCompare(agentDisplayName(right)));
  }, [allAgents, globalSkills, projectSkills, isProjectSelected]);

  const agentDisplayNames = useMemo(
    () => new Map(allAgents.map((agent) => [agentId(agent), agentDisplayName(agent)])),
    [allAgents]
  );

  const updatingSkills = useMemo<Map<string, SkillUpdateDisplayStatus>>(() => {
    const activeUpdatePhase = updatePhase === 'acquiring'
      || updatePhase === 'validating'
      || updatePhase === 'updating'
      ? updatePhase
      : null;
    if (!activeUpdatePhase || !updateContext) return new Map();
    const updateContextKey = contextKey(updateContext);
    const result = new Map<string, SkillUpdateDisplayStatus>();
    for (const skill of [...globalSkills, ...projectSkills]) {
      const skillContextKey = skill.scope === 'project' ? selectedContextKey : globalContextKey;
      if (skillContextKey !== updateContextKey || !updateSkillNames.includes(skill.name)) continue;
      result.set(getSkillIdentityKey({
        name: skill.name,
        scope: skill.scope,
        projectPath: skill.scope === 'project' ? projectPath : undefined,
      }), activeUpdatePhase);
    }
    return result;
  }, [globalContextKey, globalSkills, projectPath, projectSkills, selectedContextKey, updateContext, updatePhase, updateSkillNames]);

  // 使用 deferredQuery 而非 searchQuery，列表过滤作为低优先级更新
  const filteredGlobalSkills = useMemo(
    () => filterSkills(globalSkills, deferredQuery, selectedAgentFilter),
    [globalSkills, deferredQuery, selectedAgentFilter]
  );

  const filteredProjectSkills = useMemo(
    () => filterSkills(projectSkills, deferredQuery, selectedAgentFilter),
    [projectSkills, deferredQuery, selectedAgentFilter]
  );

  const conflictSkillNames = useMemo(() => {
    const globalNames = new Set(globalSkills.map((s) => s.name));
    const conflicts = new Set<string>();
    for (const skill of projectSkills) {
      if (globalNames.has(skill.name)) {
        conflicts.add(skill.name);
      }
    }
    return conflicts;
  }, [globalSkills, projectSkills]);

  const handleDeleteGlobal = useCallback((skill: InstalledSkill) => {
    void openSkillRemoval(skill, selectedGlobalContext);
  }, [selectedGlobalContext]);

  const handleDeleteProject = useCallback((skill: InstalledSkill) => {
    void openSkillRemoval(skill, selectedContext, projectPath);
  }, [projectPath, selectedContext]);

  const handleManageAgentsGlobal = useCallback((skill: InstalledSkill) => {
    void openManageAgentChanges(skill, selectedGlobalContext);
  }, [selectedGlobalContext]);

  const handleManageAgentsProject = useCallback((skill: InstalledSkill) => {
    void openManageAgentChanges(skill, selectedContext, projectPath);
  }, [projectPath, selectedContext]);

  const handleAddGlobal = useCallback(() => {
    openAdd(selectedGlobalContext);
  }, [openAdd, selectedGlobalContext]);

  const handleAddProject = useCallback(() => {
    openAdd(selectedContext, projectPath);
  }, [openAdd, projectPath, selectedContext]);

  const handleRepairGlobal = useCallback((skill: InstalledSkill) => {
    openRepairSource(skill, selectedGlobalContext);
  }, [openRepairSource, selectedGlobalContext]);

  const handleRepairProject = useCallback((skill: InstalledSkill) => {
    openRepairSource(skill, selectedContext, projectPath);
  }, [openRepairSource, projectPath, selectedContext]);

  const handleCopyToProject = useCallback((skill: InstalledSkill) => {
    openCopyToProject(skill, selectedContext);
  }, [openCopyToProject, selectedContext]);

  const handleCheckProjectUpdates = useCallback(() => {
    return forceCheckUpdates(selectedContext, { kind: 'all' });
  }, [forceCheckUpdates, selectedContext]);

  const handleCheckGlobalUpdates = useCallback(() => {
    return forceCheckUpdates(selectedGlobalContext, { kind: 'all' });
  }, [forceCheckUpdates, selectedGlobalContext]);

  const handleSync = useCallback(() => syncSkills(selectedContext), [selectedContext, syncSkills]);
  const handlePrepareGlobalUpdate = useCallback(
    (skillNames: string[], batch: boolean) => openUpdate(
      selectedGlobalContext,
      skillNames,
      batch,
    ),
    [openUpdate, selectedGlobalContext],
  );
  const handlePrepareProjectUpdate = useCallback(
    (skillNames: string[], batch: boolean) => openUpdate(selectedContext, skillNames, batch),
    [openUpdate, selectedContext],
  );

  // 缓存 emptyState JSX (rerender-memo-with-default-value)
  const projectEmptyState = useMemo(
    () => <ProjectEmptyState onAdd={handleAddProject} />,
    [handleAddProject]
  );
  const globalEmptyState = useMemo(
    () => <GlobalEmptyState onAdd={handleAddGlobal} />,
    [handleAddGlobal]
  );

  // Loading state
  if (loading) {
    return (
      <div className="flex flex-col h-full overflow-hidden bg-panel animate-in fade-in duration-300">
        <div className={compact ? 'px-3 sm:px-4 pt-3 sm:pt-4 pb-2 flex-shrink-0' : 'px-4 sm:px-6 pt-4 sm:pt-5'}>
          <div className="flex items-center gap-2 mb-3">
            <Skeleton className="h-9 flex-1" />
            <Skeleton className="h-9 w-20 hidden sm:block" />
          </div>
        </div>
        <div className="flex-1 overflow-hidden px-4 sm:px-6 pb-4 sm:pb-5 space-y-3">
          {Array.from({ length: compact ? 6 : 4 }).map((_, i) => (
            <div key={i} className="flex gap-3 sm:gap-4 p-3 sm:p-4 rounded-xl border border-border/40 bg-surface/50">
              <Skeleton className="h-10 w-10 sm:h-12 sm:w-12 rounded-lg shrink-0" />
              <div className="flex-1 space-y-2.5 min-w-0 py-1">
                <Skeleton className="h-4 w-1/3 max-w-[150px]" />
                <Skeleton className="h-3 w-5/6 max-w-[300px]" />
                {!compact && (
                  <div className="flex gap-2 pt-1.5 hidden sm:flex">
                    <Skeleton className="h-5 w-12 rounded-md" />
                    <Skeleton className="h-5 w-16 rounded-md" />
                  </div>
                )}
              </div>
            </div>
          ))}
        </div>
      </div>
    );
  }

  // Error state
  if (error) {
    return (
      <div className="flex h-full items-center justify-center px-6">
        <div role="alert" className="max-w-md text-center">
          <p className="text-sm font-medium text-foreground">{t('skills.loadError')}</p>
          <p className="mt-1 text-xs text-destructive">{formatAppError(error, t)}</p>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="mt-4"
            onClick={() => void refreshWorkspace(selectedContext)}
          >
            <RefreshCw className="size-4" aria-hidden="true" />
            {t('skills.retry')}
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full overflow-hidden bg-panel">
      {/* Toolbar — compact 模式下只显示搜索框 */}
      <div className={compact ? 'px-3 sm:px-4 pt-3 sm:pt-4 pb-2 flex-shrink-0' : 'px-4 sm:px-6 pt-4 sm:pt-5'}>
        <SkillsToolbar
          compact={compact}
          searchQuery={searchQuery}
          onSearchChange={setSearchQuery}
          selectedAgent={selectedAgentFilter}
          onAgentChange={setSelectedAgentFilter}
          filterableAgents={filterableAgents}
          onSync={handleSync}
          isSyncing={isSyncing}
        />
      </div>

      <CrossStorageWarningBanner />

      {/* Skills list content */}
      {compact ? (
        /* 紧凑列表 — 选中 skill 时 */
        <CompactSkillList
          globalSkills={filteredGlobalSkills}
          projectSkills={filteredProjectSkills}
          selectedSkillRef={selectedSkillRef}
          isProjectSelected={isProjectSelected}
          projectTitle={t('skills.projectSkills')}
          projectPath={projectPath}
          pathExists={projectPathExists}
          onAddProject={handleAddProject}
          onAddGlobal={handleAddGlobal}
          onSkillClick={selectSkill}
        />
      ) : (
        /* 卡片列表 — 未选中时 */
        <div className="flex-1 overflow-auto px-4 sm:px-6 pb-4 sm:pb-5">
          {/* Project Skills Section (only when project is selected) */}
          {isProjectSelected && (
            <SkillsSection
              title={t('skills.projectSkills')}
              skills={filteredProjectSkills}
              scope="project"
              conflictSkillNames={conflictSkillNames}
              pathExists={projectPathExists}
              projectPath={projectPath}
              updatingSkills={updatingSkills}
              isCheckingUpdates={isCheckingProject}
              updateCheck={projectSnapshot.updateCheck}
              agentDisplayNames={agentDisplayNames}
              auditCache={auditCache}
              onSkillClick={selectSkill}
              onPrepareUpdate={handlePrepareProjectUpdate}
              onDelete={handleDeleteProject}
              onCopyToProject={handleCopyToProject}
              onManageAgents={handleManageAgentsProject}
              onRepairSource={handleRepairProject}
              onAdd={handleAddProject}
              onCheckUpdates={handleCheckProjectUpdates}
              emptyState={projectEmptyState}
            />
          )}

          {/* Global Skills Section */}
          <SkillsSection
            title={t('skills.globalSkills')}
            skills={filteredGlobalSkills}
            scope="global"
            conflictSkillNames={conflictSkillNames}
            updatingSkills={updatingSkills}
            isCheckingUpdates={isCheckingGlobal}
            updateCheck={globalSnapshot.updateCheck}
            agentDisplayNames={agentDisplayNames}
            auditCache={auditCache}
            onSkillClick={selectSkill}
            onPrepareUpdate={handlePrepareGlobalUpdate}
            onDelete={handleDeleteGlobal}
            onManageAgents={handleManageAgentsGlobal}
            onRepairSource={handleRepairGlobal}
            onAdd={handleAddGlobal}
            onCheckUpdates={handleCheckGlobalUpdates}
            emptyState={globalEmptyState}
          />
        </div>
      )}

      <DeleteSkillDialog />
      <RepairSourceDialog />
    </div>
  );
}
