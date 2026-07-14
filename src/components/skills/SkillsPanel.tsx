// src/components/skills/SkillsPanel.tsx
import { useState, useEffect, useMemo, useCallback, useDeferredValue, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useContextStore } from '@/stores/context';
import { useSkillsDataStore } from '@/stores/skills-data';
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
import type { AgentType, InstalledSkill } from '@/bindings';

/** 按搜索关键词 + agent 筛选过滤 skills — 单次遍历 (js-combine-iterations) */
function filterSkills<T extends InstalledSkill>(skills: T[], searchQuery: string, agentFilter: string): T[] {
  if (!searchQuery && agentFilter === 'all') return skills;
  const query = searchQuery ? searchQuery.toLowerCase() : '';
  return skills.filter((s) => {
    if (query && !s.name.toLowerCase().includes(query) && !s.description.toLowerCase().includes(query)) {
      return false;
    }
    if (agentFilter !== 'all' && !s.agents.includes(agentFilter as AgentType)) {
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
  const { selectedContext, selectedContextRef, hasExplicitContext } = useContextStore();
  const selectedContextKey = hasExplicitContext
    ? JSON.stringify(selectedContextRef)
    : selectedContext;
  const globalContextKey = hasExplicitContext
    ? JSON.stringify({
      environment: selectedContextRef.environment,
      scope: { scope: 'global' },
    })
    : 'global';
  const projectContextKey = hasExplicitContext && selectedContextRef.scope.scope === 'project'
    ? selectedContextKey
    : selectedContext;

  // ① Store — 细粒度 selector 订阅
  const globalSkills = useSkillsDataStore((s) => s.globalSkills);
  const projectSkills = useSkillsDataStore((s) => s.projectSkills);
  const projectPathExists = useSkillsDataStore((s) => s.projectPathExists);
  const allAgents = useSkillsDataStore((s) => s.allAgents);
  const loading = useSkillsDataStore((s) => s.loading);
  const error = useSkillsDataStore((s) => s.error);
  const isSyncing = useSkillsDataStore((s) => s.isSyncing);
  const isCheckingGlobal = useSkillsDataStore((s) => s.checkingUpdateScopes.has(globalContextKey));
  const isCheckingProject = useSkillsDataStore((s) => s.checkingUpdateScopes.has(projectContextKey));
  const syncUpdates = useSkillsDataStore((s) => s.syncUpdates);
  const forceCheckUpdates = useSkillsDataStore((s) => s.forceCheckUpdates);
  const updatingSkills = useSkillsDataStore((s) => s.updatingSkills);
  const updateAllInSection = useSkillsDataStore((s) => s.updateAllInSection);
  const cancelUpdateAll = useSkillsDataStore((s) => s.cancelUpdateAll);
  const fetchSkills = useSkillsDataStore((s) => s.fetchSkills);
  const syncSkills = useSkillsDataStore((s) => s.syncSkills);
  const storeUpdateSkill = useSkillsDataStore((s) => s.updateSkill);
  const auditCache = useSkillsDataStore((s) => s.auditCache);
  const fetchAuditForSkills = useSkillsDataStore((s) => s.fetchAuditForSkills);
  const selectSkill = useSkillDetailStore((s) => s.selectSkill);
  const deselectSkill = useSkillDetailStore((s) => s.deselectSkill);
  const selectedSkillRef = useSkillDetailStore((s) => s.selectedSkillRef);
  const openDelete = useSkillDialogStore((s) => s.openDelete);
  const openAdd = useSkillDialogStore((s) => s.openAdd);
  const openRepairSource = useSkillDialogStore((s) => s.openRepairSource);
  const openCopyToProject = useSkillDialogStore((s) => s.openCopyToProject);
  const openManageAgents = useSkillDialogStore((s) => s.openManageAgents);

  // ② UI 状态 — 仅 2 个 useState
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedAgentFilter, setSelectedAgentFilter] = useState('all');

  // 搜索优化：列表过滤作为低优先级更新 (rerender-transitions)
  const deferredQuery = useDeferredValue(searchQuery);

  // ③ 数据初始化 — mount / selectedContext 变化时重新获取，然后自动检测更新
  useEffect(() => {
    let ignore = false;
    fetchSkills().then(() => {
      if (!ignore) syncUpdates(); // 后台检测更新，不阻塞 UI
    });
    return () => { ignore = true; };
  }, [selectedContextKey, fetchSkills, syncUpdates]);

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

  // ④ Derived state
  const isProjectSelected = hasExplicitContext
    ? selectedContextRef.scope.scope === 'project'
    : selectedContext !== 'global';

  const filterableAgents = useMemo(() => {
    const agentIds = new Set<string>();
    const allSkills = isProjectSelected ? [...globalSkills, ...projectSkills] : globalSkills;
    for (const s of allSkills) {
      for (const id of s.agents) agentIds.add(id);
    }
    return allAgents
      .filter((a) => agentIds.has(a.id))
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [allAgents, globalSkills, projectSkills, isProjectSelected]);

  const agentDisplayNames = useMemo(
    () => new Map(allAgents.map((a) => [a.id, a.name])),
    [allAgents]
  );

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
    openDelete(skill, 'global');
  }, [openDelete]);

  const handleDeleteProject = useCallback((skill: InstalledSkill) => {
    openDelete(skill, 'project', selectedContext);
  }, [openDelete, selectedContext]);

  const handleManageAgentsGlobal = useCallback((skill: InstalledSkill) => {
    openManageAgents(skill, 'global');
  }, [openManageAgents]);

  const handleManageAgentsProject = useCallback((skill: InstalledSkill) => {
    openManageAgents(skill, 'project');
  }, [openManageAgents]);

  const handleAddGlobal = useCallback(() => {
    openAdd('global');
  }, [openAdd]);

  const handleAddProject = useCallback(() => {
    openAdd('project');
  }, [openAdd]);

  const handleRepairGlobal = useCallback((skill: InstalledSkill) => {
    openRepairSource(skill, 'global');
  }, [openRepairSource]);

  const handleRepairProject = useCallback((skill: InstalledSkill) => {
    openRepairSource(skill, 'project', selectedContext);
  }, [openRepairSource, selectedContext]);

  const handleCheckProjectUpdates = useCallback(() => {
    return forceCheckUpdates('project');
  }, [forceCheckUpdates]);

  const handleCheckGlobalUpdates = useCallback(() => {
    return forceCheckUpdates('global');
  }, [forceCheckUpdates]);

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
      <div className="flex items-center justify-center h-full">
        <div className="text-destructive">{error}</div>
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
          onSync={syncSkills}
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
          projectPath={selectedContext}
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
              projectPath={selectedContext}
              updatingSkills={updatingSkills}
              isCheckingUpdates={isCheckingProject}
              agentDisplayNames={agentDisplayNames}
              auditCache={auditCache}
              onSkillClick={selectSkill}
              onUpdate={storeUpdateSkill}
              onUpdateAll={updateAllInSection}
              onCancelUpdateAll={cancelUpdateAll}
              onDelete={handleDeleteProject}
              onCopyToProject={openCopyToProject}
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
            agentDisplayNames={agentDisplayNames}
            auditCache={auditCache}
            onSkillClick={selectSkill}
            onUpdate={storeUpdateSkill}
            onUpdateAll={updateAllInSection}
            onCancelUpdateAll={cancelUpdateAll}
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
