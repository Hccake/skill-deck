// src/components/skills/SkillsPanel.tsx
import { useState, useEffect, useLayoutEffect, useMemo, useCallback, useDeferredValue, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { RefreshCw } from 'lucide-react';
import { useWorkspaceContextStore } from '@/stores/workspace-context';
import { useProjectWorkspace } from '@/hooks/useProjectWorkspace';
import {
  sourceDiagnosticsForEnvironment,
  useSkillsDataStore,
  type ContextSkillSnapshot,
} from '@/stores/skills-data';
import { useSkillDetailStore } from '@/stores/skill-detail';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { SkillsToolbar } from './SkillsToolbar';
import { SkillsSection } from './SkillsSection';
import { CompactSkillList } from './CompactSkillList';
import { CrossStorageWarningBanner } from './CrossStorageWarningBanner';
import { DeleteSkillDialog } from './DeleteSkillDialog';
import { RepairSourceDialog } from './RepairSourceDialog';
import { GlobalEmptyState, ProjectEmptyState, SkillFilterEmptyState } from './EmptyStates';
import { Skeleton } from '@/components/ui/skeleton';
import { Button } from '@/components/ui/button';
import { contextKey, globalContext } from '@/lib/context';
import { agentDisplayName, agentId } from '@/lib/agents';
import { formatAppError } from '@/utils/format-app-error';
import { openSkillRemoval } from '@/workflows/skill-remove';
import { useSkillUpdateWorkflow } from '@/workflows/skill-update';
import { getSkillIdentityKey } from '@/lib/skills/identity';
import {
  countSkillsByAgent,
  filterSkills,
  getAgentFilterOptions,
} from '@/lib/skills/filter';
import {
  hasCommittedUpdateComparison,
  type SkillUpdateDisplayStatus,
} from '@/stores/skills-utils';
import type { AgentId, InstalledSkill, ResolvedAgent } from '@/bindings';

const EMPTY_SNAPSHOT: ContextSkillSnapshot = {
  skills: [],
  agents: [],
  pathExists: true,
  loading: false,
  error: null,
  requestId: 0,
};
const EMPTY_SKILL_NAMES: string[] = [];

function hasCommittedComparison(snapshot: ContextSkillSnapshot): boolean {
  return snapshot.skills.some(hasCommittedUpdateComparison);
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
  const { projects } = useProjectWorkspace(selectedContext.environment);
  const selectedScope = selectedContext.scope;
  const selectedProject = selectedScope.scope === 'project'
    ? projects.find((project) => project.binding.id === selectedScope.project_id)
    : null;
  const projectPath = selectedProject?.binding.nativePath;

  // ① Store — 细粒度 selector 订阅
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
  const projectSkills = isProjectSelected ? projectSnapshot.skills : EMPTY_SNAPSHOT.skills;
  const projectPathExists = projectSnapshot.pathExists;
  const loading = (globalSnapshot.loading && globalSkills.length === 0)
    || (isProjectSelected && projectSnapshot.loading && projectSkills.length === 0);
  const error = projectSnapshot.error ?? globalSnapshot.error;
  const isSyncing = useSkillsDataStore((s) => s.isSyncing);
  const isAutomaticCheckingGlobal = useSkillsDataStore((s) => (
    s.automaticUpdateScopes?.has(globalContextKey)
      ?? s.checkingUpdateScopes.has(globalContextKey)
  ));
  const isAutomaticCheckingProject = useSkillsDataStore((s) => (
    projectContextKey
      ? (s.automaticUpdateScopes?.has(projectContextKey) ?? s.checkingUpdateScopes.has(projectContextKey))
      : false
  ));
  const isForceCheckingGlobal = useSkillsDataStore((s) => s.forceUpdateScopes?.has(globalContextKey) ?? false);
  const isForceCheckingProject = useSkillsDataStore((s) => (
    projectContextKey ? s.forceUpdateScopes?.has(projectContextKey) ?? false : false
  ));
  const activateAutomaticChecks = useSkillsDataStore((s) => s.activateAutomaticChecks ?? s.syncUpdates);
  const forceCheckUpdates = useSkillsDataStore((s) => s.forceCheckUpdates);
  const activeUpdatePhase = useSkillUpdateWorkflow((s) => (
    s.phase === 'executing' ? 'updating' : null
  ));
  const activeUpdateContext = useSkillUpdateWorkflow((s) => (
    s.phase === 'executing' ? s.context : null
  ));
  const activeUpdateSkillNames = useSkillUpdateWorkflow((s) => (
    s.phase === 'executing' ? s.skillNames : EMPTY_SKILL_NAMES
  ));
  const openUpdate = useSkillUpdateWorkflow((s) => s.open);
  const refreshWorkspace = useSkillsDataStore((s) => s.refreshWorkspace);
  const syncSkills = useSkillsDataStore((s) => s.syncSkills);
  const selectSkill = useSkillDetailStore((s) => s.selectSkill);
  const deselectSkill = useSkillDetailStore((s) => s.deselectSkill);
  const selectedSkillRef = useSkillDetailStore((s) => s.selectedSkillRef);
  const openAdd = useSkillDialogStore((s) => s.openAdd);
  const openRepairSource = useSkillDialogStore((s) => s.openRepairSource);
  const openCopyToProject = useSkillDialogStore((s) => s.openCopyToProject);
  const openManageAgents = useSkillDialogStore((s) => s.openManageAgents);

  // ② UI 状态 — 仅 2 个 useState
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedAgentFilter, setSelectedAgentFilter] = useState<AgentId | null>(null);
  const listScrollRef = useRef<HTMLDivElement>(null);

  // 搜索优化：列表过滤作为低优先级更新 (rerender-transitions)
  const deferredQuery = useDeferredValue(searchQuery);

  useLayoutEffect(() => {
    if (listScrollRef.current) listScrollRef.current.scrollTop = 0;
  }, [selectedContextKey]);

  // 长生命周期 store 会在 Context snapshot 加载后统一决定是否准入 Automatic。
  // 组件不监听 focus，也不在重新挂载时安排 timer；同一应用会话返回页面不得新增 IPC 请求。
  useEffect(() => {
    let ignore = false;
    void refreshWorkspace(selectedContext).then(() => {
      if (!ignore) void activateAutomaticChecks(selectedContext);
    });
    return () => { ignore = true; };
  }, [selectedContext, selectedContextKey, refreshWorkspace, activateAutomaticChecks]);

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

  const filterableAgents = useMemo(() => {
    const agentsById = new Map<AgentId, ResolvedAgent>();
    for (const agent of getAgentFilterOptions(globalSnapshot.agents, selectedAgentFilter)) {
      agentsById.set(agentId(agent), agent);
    }
    for (const agent of getAgentFilterOptions(projectSnapshot.agents, selectedAgentFilter)) {
      if (!agentsById.has(agentId(agent))) {
        agentsById.set(agentId(agent), agent);
      }
    }
    return [...agentsById.values()].sort((left, right) => (
      agentDisplayName(left).localeCompare(agentDisplayName(right))
    ));
  }, [globalSnapshot.agents, projectSnapshot.agents, selectedAgentFilter]);

  const availableAgentIds = useMemo(
    () => new Set(filterableAgents.map((agent) => agentId(agent))),
    [filterableAgents],
  );
  const isAgentFilterDataLoading = globalSnapshot.loading
    || globalSnapshot.requestId === 0
    || (isProjectSelected && (
      projectSnapshot.loading
      || projectSnapshot.requestId === 0
    ));
  const activeAgentFilter = selectedAgentFilter !== null
    && (availableAgentIds.has(selectedAgentFilter) || isAgentFilterDataLoading)
    ? selectedAgentFilter
    : null;

  useEffect(() => {
    if (
      selectedAgentFilter !== null
      && !isAgentFilterDataLoading
      && !availableAgentIds.has(selectedAgentFilter)
    ) {
      const resetTimer = setTimeout(() => setSelectedAgentFilter(null), 0);
      return () => clearTimeout(resetTimer);
    }
    return undefined;
  }, [availableAgentIds, isAgentFilterDataLoading, selectedAgentFilter]);

  const agentDisplayNames = useMemo(
    () => new Map(
      [...globalSnapshot.agents, ...projectSnapshot.agents]
        .map((agent) => [agentId(agent), agentDisplayName(agent)]),
    ),
    [globalSnapshot.agents, projectSnapshot.agents]
  );

  const agentMatchCounts = useMemo(() => {
    const counts = countSkillsByAgent([...globalSkills, ...projectSkills]);
    return counts;
  }, [globalSkills, projectSkills]);

  const updatingSkills = useMemo<Map<string, SkillUpdateDisplayStatus>>(() => {
    if (!activeUpdatePhase || !activeUpdateContext) return new Map();
    const updateContextKey = contextKey(activeUpdateContext);
    const result = new Map<string, SkillUpdateDisplayStatus>();
    for (const skill of [...globalSkills, ...projectSkills]) {
      const skillContextKey = skill.scope === 'project' ? selectedContextKey : globalContextKey;
      if (skillContextKey !== updateContextKey || !activeUpdateSkillNames.includes(skill.name)) continue;
      result.set(getSkillIdentityKey({
        name: skill.name,
        scope: skill.scope,
        projectPath: skill.scope === 'project' ? projectPath : undefined,
      }), activeUpdatePhase);
    }
    return result;
  }, [activeUpdateContext, activeUpdatePhase, activeUpdateSkillNames, globalContextKey, globalSkills, projectPath, projectSkills, selectedContextKey]);

  // 使用 deferredQuery 而非 searchQuery，列表过滤作为低优先级更新
  const filteredGlobalSkills = useMemo(
    () => filterSkills(globalSkills, deferredQuery, activeAgentFilter),
    [activeAgentFilter, deferredQuery, globalSkills]
  );

  const filteredProjectSkills = useMemo(
    () => filterSkills(projectSkills, deferredQuery, activeAgentFilter),
    [activeAgentFilter, deferredQuery, projectSkills]
  );

  const totalSkillCount = globalSkills.length + projectSkills.length;
  const hasActiveFilters = Boolean(deferredQuery.trim()) || activeAgentFilter !== null;

  const selectedAgentName = activeAgentFilter
    ? agentDisplayNames.get(activeAgentFilter) ?? activeAgentFilter
    : undefined;

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
    openManageAgents(skill, selectedGlobalContext);
  }, [openManageAgents, selectedGlobalContext]);

  const handleManageAgentsProject = useCallback((skill: InstalledSkill) => {
    openManageAgents(skill, selectedContext);
  }, [openManageAgents, selectedContext]);

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

  const handleSync = useCallback(
    () => syncSkills(selectedContext, { origin: 'passive' }),
    [selectedContext, syncSkills],
  );
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
  const projectInstalledEmptyState = useMemo(
    () => <ProjectEmptyState onAdd={handleAddProject} />,
    [handleAddProject]
  );
  const globalInstalledEmptyState = useMemo(
    () => <GlobalEmptyState onAdd={handleAddGlobal} />,
    [handleAddGlobal]
  );
  const projectFilterEmptyState = hasActiveFilters && projectSkills.length > 0 && filteredProjectSkills.length === 0
    ? (
      <SkillFilterEmptyState
        agentName={selectedAgentName}
        searchQuery={deferredQuery}
      />
    )
    : undefined;
  const globalFilterEmptyState = hasActiveFilters && globalSkills.length > 0 && filteredGlobalSkills.length === 0
    ? (
      <SkillFilterEmptyState
        agentName={selectedAgentName}
        searchQuery={deferredQuery}
      />
    )
    : undefined;
  const projectEmptyState = projectFilterEmptyState ?? projectInstalledEmptyState;
  const globalEmptyState = globalFilterEmptyState ?? globalInstalledEmptyState;

  const handleCheckProjectUpdates = useCallback(() => {
    const selection = hasActiveFilters
      ? {
        kind: 'skills' as const,
        skills: filteredProjectSkills.map((skill) => ({ context: selectedContext, skillName: skill.name })),
      }
      : { kind: 'all' as const };
    return forceCheckUpdates(selectedContext, selection);
  }, [filteredProjectSkills, forceCheckUpdates, hasActiveFilters, selectedContext]);

  const handleCheckGlobalUpdates = useCallback(() => {
    const selection = hasActiveFilters
      ? {
        kind: 'skills' as const,
        skills: filteredGlobalSkills.map((skill) => ({ context: selectedGlobalContext, skillName: skill.name })),
      }
      : { kind: 'all' as const };
    return forceCheckUpdates(selectedGlobalContext, selection);
  }, [filteredGlobalSkills, forceCheckUpdates, hasActiveFilters, selectedGlobalContext]);

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
            onClick={() => {
              void refreshWorkspace(selectedContext).then(() => activateAutomaticChecks(selectedContext));
            }}
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
      {/* Toolbar — compact 模式也保留搜索与 Agent 筛选 */}
      <div className={compact ? 'px-3 sm:px-4 pt-3 sm:pt-4 pb-2 flex-shrink-0' : 'px-4 sm:px-6 pt-4 sm:pt-5'}>
        <SkillsToolbar
          compact={compact}
          searchQuery={searchQuery}
          onSearchChange={setSearchQuery}
          selectedAgent={activeAgentFilter}
          onAgentChange={setSelectedAgentFilter}
          filterableAgents={filterableAgents}
          agentMatchCounts={agentMatchCounts}
          totalSkillCount={totalSkillCount}
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
          projectEmptyState={projectFilterEmptyState}
          globalEmptyState={globalFilterEmptyState}
        />
      ) : (
        /* 卡片列表 — 未选中时 */
        <div ref={listScrollRef} className="flex-1 overflow-auto px-4 sm:px-6 pb-4 sm:pb-5">
          {/* Project Skills Section (only when project is selected) */}
          {isProjectSelected && (
            <SkillsSection
              title={t('skills.projectSkills')}
              skills={filteredProjectSkills}
              sourceDiagnostics={environmentSourceDiagnostics}
              scope="project"
              filterActive={hasActiveFilters}
              conflictSkillNames={conflictSkillNames}
              pathExists={projectPathExists}
              projectPath={projectPath}
              updatingSkills={updatingSkills}
              isCheckingUpdates={isForceCheckingProject}
              isAutomaticCheckingUpdates={isAutomaticCheckingProject}
              hasCommittedComparison={hasCommittedComparison(projectSnapshot)}
              agentDisplayNames={agentDisplayNames}
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
            sourceDiagnostics={environmentSourceDiagnostics}
            scope="global"
            filterActive={hasActiveFilters}
            conflictSkillNames={conflictSkillNames}
            updatingSkills={updatingSkills}
            isCheckingUpdates={isForceCheckingGlobal}
            isAutomaticCheckingUpdates={isAutomaticCheckingGlobal}
            hasCommittedComparison={hasCommittedComparison(globalSnapshot)}
            agentDisplayNames={agentDisplayNames}
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
