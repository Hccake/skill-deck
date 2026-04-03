// src/components/skills/SkillsPanel.tsx
import { useState, useEffect, useMemo, useCallback, useDeferredValue, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useContextStore } from '@/stores/context';
import { useSkillsStore } from '@/stores/skills';
import { SkillsToolbar } from './SkillsToolbar';
import { SkillsSection } from './SkillsSection';
import { CompactSkillList } from './CompactSkillList';
import { DeleteSkillDialog } from './DeleteSkillDialog';
import { GlobalEmptyState, ProjectEmptyState } from './EmptyStates';
import type { AgentType, InstalledSkill } from '@/bindings';

/** 按搜索关键词 + agent 筛选过滤 skills — 单次遍历 (js-combine-iterations) */
function filterSkills(skills: InstalledSkill[], searchQuery: string, agentFilter: string): InstalledSkill[] {
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
  const { selectedContext } = useContextStore();

  // ① Store — 细粒度 selector 订阅
  const globalSkills = useSkillsStore((s) => s.globalSkills);
  const projectSkills = useSkillsStore((s) => s.projectSkills);
  const projectPathExists = useSkillsStore((s) => s.projectPathExists);
  const allAgents = useSkillsStore((s) => s.allAgents);
  const loading = useSkillsStore((s) => s.loading);
  const error = useSkillsStore((s) => s.error);
  const isSyncing = useSkillsStore((s) => s.isSyncing);
  const isCheckingGlobal = useSkillsStore((s) => s.checkingUpdateScopes.has('global'));
  const isCheckingProject = useSkillsStore((s) => s.checkingUpdateScopes.has(selectedContext));
  const syncUpdates = useSkillsStore((s) => s.syncUpdates);
  const forceCheckUpdates = useSkillsStore((s) => s.forceCheckUpdates);
  const updatingSkills = useSkillsStore((s) => s.updatingSkills);
  const updateAllInSection = useSkillsStore((s) => s.updateAllInSection);
  const cancelUpdateAll = useSkillsStore((s) => s.cancelUpdateAll);
  const fetchSkills = useSkillsStore((s) => s.fetchSkills);
  const syncSkills = useSkillsStore((s) => s.syncSkills);
  const storeUpdateSkill = useSkillsStore((s) => s.updateSkill);
  const selectSkill = useSkillsStore((s) => s.selectSkill);
  const deselectSkill = useSkillsStore((s) => s.deselectSkill);
  const selectedSkill = useSkillsStore((s) => s.selectedSkill);
  const openDelete = useSkillsStore((s) => s.openDelete);
  const openAdd = useSkillsStore((s) => s.openAdd);
  const auditCache = useSkillsStore((s) => s.auditCache);
  const fetchAuditForSkills = useSkillsStore((s) => s.fetchAuditForSkills);

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
  }, [selectedContext, fetchSkills, syncUpdates]);

  // ③a 仅在 context 真正切换时关闭详情面板
  const previousContextRef = useRef(selectedContext);
  useEffect(() => {
    if (previousContextRef.current !== selectedContext) {
      deselectSkill();
      previousContextRef.current = selectedContext;
    }
  }, [selectedContext, deselectSkill]);

  // Esc 键关闭详情面板
  useEffect(() => {
    if (!selectedSkill) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') deselectSkill();
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [selectedSkill, deselectSkill]);

  // ③b 审计数据 — skills 变化后获取（仅对有 source 的 skills 请求）
  useEffect(() => {
    const allSkills = [...globalSkills, ...projectSkills];
    const skillsWithSource = allSkills.filter((s) => s.source);
    if (skillsWithSource.length > 0) {
      fetchAuditForSkills(skillsWithSource);
    }
  }, [globalSkills, projectSkills, fetchAuditForSkills]);

  // ④ Derived state
  const isProjectSelected = selectedContext !== 'global';

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

  const handleAddGlobal = useCallback(() => {
    openAdd('global');
  }, [openAdd]);

  const handleAddProject = useCallback(() => {
    openAdd('project');
  }, [openAdd]);

  const handleCheckProjectUpdates = useCallback(() => {
    forceCheckUpdates('project');
  }, [forceCheckUpdates]);

  const handleCheckGlobalUpdates = useCallback(() => {
    forceCheckUpdates('global');
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
      <div className="flex items-center justify-center h-full">
        <div className="text-muted-foreground">{t('common.loading')}</div>
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

      {/* Skills list content */}
      {compact ? (
        /* 紧凑列表 — 选中 skill 时 */
        <CompactSkillList
          globalSkills={filteredGlobalSkills}
          projectSkills={filteredProjectSkills}
          selectedSkillName={selectedSkill?.name ?? null}
          selectedSkillScope={selectedSkill?.scope ?? null}
          isProjectSelected={isProjectSelected}
          projectTitle={t('skills.projectSkills')}
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
            onAdd={handleAddGlobal}
            onCheckUpdates={handleCheckGlobalUpdates}
            emptyState={globalEmptyState}
          />
        </div>
      )}

      <DeleteSkillDialog />
    </div>
  );
}
