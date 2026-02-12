// src/components/skills/SkillsPanel.tsx
import { useState, useEffect, useMemo, useCallback, useDeferredValue } from 'react';
import { useTranslation } from 'react-i18next';
import { useContextStore } from '@/stores/context';
import { useSkillsStore } from '@/stores/skills';
import { SkillsToolbar } from './SkillsToolbar';
import { SkillsSection } from './SkillsSection';
import { SkillDetailDialog } from './SkillDetailDialog';
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

export function SkillsPanel() {
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
  const updatingSkill = useSkillsStore((s) => s.updatingSkill);
  const fetchSkills = useSkillsStore((s) => s.fetchSkills);
  const syncSkills = useSkillsStore((s) => s.syncSkills);
  const storeUpdateSkill = useSkillsStore((s) => s.updateSkill);
  const openDetail = useSkillsStore((s) => s.openDetail);
  const openDelete = useSkillsStore((s) => s.openDelete);
  const openAdd = useSkillsStore((s) => s.openAdd);

  // ② UI 状态 — 仅 2 个 useState
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedAgentFilter, setSelectedAgentFilter] = useState('all');

  // 搜索优化：列表过滤作为低优先级更新 (rerender-transitions)
  const deferredQuery = useDeferredValue(searchQuery);

  // ③ 数据初始化 — selectedContext 变化时重新获取
  useEffect(() => {
    fetchSkills();
  }, [selectedContext, fetchSkills]);

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

  // ⑤ Event handlers — 直接调用 store action，无需 useCallback 包装
  const handleToggleAgent = useCallback((skillName: string, agentId: string) => {
    console.log('TODO: Toggle agent', skillName, agentId);
  }, []);

  const handleDeleteGlobal = useCallback((skillName: string) => {
    openDelete(skillName, 'global');
  }, [openDelete]);

  const handleDeleteProject = useCallback((skillName: string) => {
    openDelete(skillName, 'project', selectedContext);
  }, [openDelete, selectedContext]);

  const handleAddGlobal = useCallback(() => {
    openAdd('global');
  }, [openAdd]);

  const handleAddProject = useCallback(() => {
    openAdd('project');
  }, [openAdd]);

  // 缓存 emptyState JSX (rerender-memo-with-default-value)
  const projectEmptyState = useMemo(() => <ProjectEmptyState />, []);
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
    <div className="flex-1 flex flex-col overflow-hidden">
      {/* Toolbar */}
      <div className="px-4 sm:px-6 pt-4 sm:pt-5">
        <SkillsToolbar
          searchQuery={searchQuery}
          onSearchChange={setSearchQuery}
          selectedAgent={selectedAgentFilter}
          onAgentChange={setSelectedAgentFilter}
          filterableAgents={filterableAgents}
          onSync={syncSkills}
          isSyncing={isSyncing}
        />
      </div>

      {/* Skills Content */}
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
            updatingSkill={updatingSkill}
            agentDisplayNames={agentDisplayNames}
            onSkillClick={openDetail}
            onUpdate={storeUpdateSkill}
            onDelete={handleDeleteProject}
            onToggleAgent={handleToggleAgent}
            onAdd={handleAddProject}
            emptyState={projectEmptyState}
          />
        )}

        {/* Global Skills Section */}
        <SkillsSection
          title={t('skills.globalSkills')}
          skills={filteredGlobalSkills}
          scope="global"
          conflictSkillNames={conflictSkillNames}
          updatingSkill={updatingSkill}
          agentDisplayNames={agentDisplayNames}
          onSkillClick={openDetail}
          onUpdate={storeUpdateSkill}
          onDelete={handleDeleteGlobal}
          onToggleAgent={handleToggleAgent}
          onAdd={handleAddGlobal}
          emptyState={globalEmptyState}
        />
      </div>

      {/* Dialog 完全自治 — 零 props，各自从 store 读取 */}
      <SkillDetailDialog />
      <DeleteSkillDialog />
    </div>
  );
}
