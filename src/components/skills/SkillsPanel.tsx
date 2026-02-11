// src/components/skills/SkillsPanel.tsx
import { useState, useEffect, useMemo, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { useContextStore } from '@/stores/context';
import { listSkills, listAgents, removeSkill, checkUpdates, updateSkill } from '@/hooks/useTauriApi';
import { SkillsToolbar } from './SkillsToolbar';
import { SkillsSection } from './SkillsSection';
import { SkillDetailDialog } from './SkillDetailDialog';
import { AddSkillDialog } from './AddSkillDialog';
import { GlobalEmptyState, ProjectEmptyState } from './EmptyStates';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import type { Agent, Skill, SkillScope, SkillUpdateInfo, AgentType } from '@/types';

export function SkillsPanel() {
  const { t } = useTranslation();
  const { selectedContext } = useContextStore();

  // State
  const [globalSkills, setGlobalSkills] = useState<Skill[]>([]);
  const [projectSkills, setProjectSkills] = useState<Skill[]>([]);
  const [projectPathExists, setProjectPathExists] = useState(true);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedAgentFilter, setSelectedAgentFilter] = useState('all');
  const [isSyncing, setIsSyncing] = useState(false);
  const [updatingSkill, setUpdatingSkill] = useState<string | null>(null);

  // All agents (for filter dropdown and display names)
  const [allAgents, setAllAgents] = useState<Agent[]>([]);

  // Detail dialog state
  const [selectedSkill, setSelectedSkill] = useState<Skill | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);

  // Add skill dialog state
  const [addDialogOpen, setAddDialogOpen] = useState(false);
  const [addDialogScope, setAddDialogScope] = useState<SkillScope>('global');

  // Delete confirmation state
  const [deleteTarget, setDeleteTarget] = useState<{
    name: string;
    scope: SkillScope;
    projectPath?: string;
  } | null>(null);
  const [isDeleting, setIsDeleting] = useState(false);

  /** 按名称排序 skills，保证展示顺序稳定 */
  const sortSkills = useCallback(
    (skills: Skill[]): Skill[] => [...skills].sort((a, b) => a.name.localeCompare(b.name)),
    []
  );

  /** 将 check_updates 结果合并到 skills 列表 */
  const mergeUpdateInfo = useCallback(
    (skills: Skill[], updates: SkillUpdateInfo[]): Skill[] => {
      const updateMap = new Map(updates.map((u) => [u.name, u.hasUpdate]));
      return skills.map((s) => ({
        ...s,
        hasUpdate: updateMap.get(s.name) ?? false,
      }));
    },
    []
  );

  // Fetch skills data
  // 使用 Promise.all 并行获取，符合 async-parallel 规则
  const fetchSkills = useCallback(async () => {
    const isProjectSelected = selectedContext !== 'global';

    try {
      setLoading(true);
      setError(null);

      if (isProjectSelected) {
        // 并行获取 agents + global + project skills
        const [agents, globalResult, projectResult] = await Promise.all([
          listAgents(),
          listSkills({ scope: 'global' }),
          listSkills({ scope: 'project', projectPath: selectedContext }),
        ]);
        setAllAgents(agents);
        setGlobalSkills(sortSkills(globalResult.skills));
        setProjectSkills(sortSkills(projectResult.skills));
        setProjectPathExists(projectResult.pathExists);
      } else {
        const [agents, globalResult] = await Promise.all([
          listAgents(),
          listSkills({ scope: 'global' }),
        ]);
        setAllAgents(agents);
        setGlobalSkills(sortSkills(globalResult.skills));
        setProjectSkills([]);
        setProjectPathExists(true);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load skills');
    } finally {
      setLoading(false);
    }
  }, [selectedContext]);

  useEffect(() => {
    fetchSkills();
  }, [fetchSkills]);

  // Sync handler
  const handleSync = useCallback(async () => {
    setIsSyncing(true);
    try {
      const isProjectSelected = selectedContext !== 'global';

      if (isProjectSelected) {
        const [globalResult, projectResult, globalUpdates, projectUpdates] =
          await Promise.all([
            listSkills({ scope: 'global' }),
            listSkills({ scope: 'project', projectPath: selectedContext }),
            checkUpdates('global').catch(() => [] as SkillUpdateInfo[]),
            checkUpdates('project', selectedContext).catch(() => [] as SkillUpdateInfo[]),
          ]);

        setGlobalSkills(sortSkills(mergeUpdateInfo(globalResult.skills, globalUpdates)));
        setProjectSkills(sortSkills(mergeUpdateInfo(projectResult.skills, projectUpdates)));
        setProjectPathExists(projectResult.pathExists);
      } else {
        const [globalResult, globalUpdates] = await Promise.all([
          listSkills({ scope: 'global' }),
          checkUpdates('global').catch(() => [] as SkillUpdateInfo[]),
        ]);

        setGlobalSkills(sortSkills(mergeUpdateInfo(globalResult.skills, globalUpdates)));
        setProjectSkills([]);
        setProjectPathExists(true);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to sync skills');
    } finally {
      setIsSyncing(false);
    }
  }, [selectedContext, mergeUpdateInfo]);

  // Derived state
  const isProjectSelected = selectedContext !== 'global';

  // All agents available for the filter dropdown — collect agents that appear in skill.agents
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

  // Agent display name map for SkillCard synced agent labels
  const agentDisplayNames = useMemo(
    () => new Map(allAgents.map((a) => [a.id, a.name])),
    [allAgents]
  );

  // Filter skills by search query + agent filter
  const filteredGlobalSkills = useMemo(() => {
    let skills = globalSkills;
    if (searchQuery) {
      const query = searchQuery.toLowerCase();
      skills = skills.filter(
        (s) =>
          s.name.toLowerCase().includes(query) ||
          s.description.toLowerCase().includes(query)
      );
    }
    if (selectedAgentFilter !== 'all') {
      skills = skills.filter((s) => s.agents.includes(selectedAgentFilter as AgentType));
    }
    return skills;
  }, [globalSkills, searchQuery, selectedAgentFilter]);

  const filteredProjectSkills = useMemo(() => {
    let skills = projectSkills;
    if (searchQuery) {
      const query = searchQuery.toLowerCase();
      skills = skills.filter(
        (s) =>
          s.name.toLowerCase().includes(query) ||
          s.description.toLowerCase().includes(query)
      );
    }
    if (selectedAgentFilter !== 'all') {
      skills = skills.filter((s) => s.agents.includes(selectedAgentFilter as AgentType));
    }
    return skills;
  }, [projectSkills, searchQuery, selectedAgentFilter]);

  // Detect conflicts — js-set-map-lookups 规则
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

  // Event handlers
  const handleUpdate = useCallback(
    async (skillName: string) => {
      if (updatingSkill) return;

      const isProjectSkill = projectSkills.some((s) => s.name === skillName);
      const scope: SkillScope = isProjectSkill ? 'project' : 'global';

      setUpdatingSkill(skillName);
      try {
        await updateSkill({
          scope,
          name: skillName,
          projectPath: isProjectSkill ? selectedContext : undefined,
        });
        toast.success(t('skills.updateSuccess', { name: skillName }));
        await handleSync();
      } catch (e) {
        toast.error(
          t('skills.updateError', {
            name: skillName,
            error: e instanceof Error ? e.message : String(e),
          })
        );
      } finally {
        setUpdatingSkill(null);
      }
    },
    [updatingSkill, projectSkills, selectedContext, handleSync, t]
  );

  const handleDeleteGlobal = useCallback((skillName: string) => {
    setDeleteTarget({ name: skillName, scope: 'global' });
  }, []);

  const handleDeleteProject = useCallback((skillName: string) => {
    setDeleteTarget({ name: skillName, scope: 'project', projectPath: selectedContext });
  }, [selectedContext]);

  const handleConfirmDelete = useCallback(async () => {
    if (!deleteTarget) return;
    setIsDeleting(true);
    try {
      await removeSkill({
        scope: deleteTarget.scope,
        name: deleteTarget.name,
        projectPath: deleteTarget.projectPath,
      });
      toast.success(t('skills.deleteSuccess', { name: deleteTarget.name }));
      await fetchSkills();
    } catch (e) {
      toast.error(t('skills.deleteError', {
        name: deleteTarget.name,
        error: e instanceof Error ? e.message : String(e),
      }));
    } finally {
      setIsDeleting(false);
      setDeleteTarget(null);
    }
  }, [deleteTarget, fetchSkills, t]);

  const handleToggleAgent = useCallback((skillName: string, agentId: string) => {
    console.log('TODO: Toggle agent', skillName, agentId);
  }, []);

  // Add skill — scope 由触发按钮所在区块决定
  const handleAddGlobal = useCallback(() => {
    setAddDialogScope('global');
    setAddDialogOpen(true);
  }, []);

  const handleAddProject = useCallback(() => {
    setAddDialogScope('project');
    setAddDialogOpen(true);
  }, []);

  // Install complete handler
  const handleInstallComplete = useCallback(() => {
    fetchSkills();
  }, [fetchSkills]);

  // Skill detail dialog handler
  const handleSkillClick = useCallback((skill: Skill) => {
    setSelectedSkill(skill);
    setDialogOpen(true);
  }, []);

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
          onSync={handleSync}
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
            onSkillClick={handleSkillClick}
            onUpdate={handleUpdate}
            onDelete={handleDeleteProject}
            onToggleAgent={handleToggleAgent}
            onAdd={handleAddProject}
            emptyState={<ProjectEmptyState />}
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
          onSkillClick={handleSkillClick}
          onUpdate={handleUpdate}
          onDelete={handleDeleteGlobal}
          onToggleAgent={handleToggleAgent}
          onAdd={handleAddGlobal}
          emptyState={<GlobalEmptyState onAdd={handleAddGlobal} />}
        />
      </div>

      {/* Skill Detail Dialog */}
      <SkillDetailDialog
        skill={selectedSkill}
        open={dialogOpen}
        onOpenChange={setDialogOpen}
      />

      {/* Add Skill Dialog — scope 固定，不可更改 */}
      <AddSkillDialog
        open={addDialogOpen}
        onOpenChange={setAddDialogOpen}
        scope={addDialogScope}
        projectPath={isProjectSelected ? selectedContext : undefined}
        onInstallComplete={handleInstallComplete}
      />

      {/* Delete Confirmation Dialog */}
      <AlertDialog open={!!deleteTarget} onOpenChange={(open) => !open && !isDeleting && setDeleteTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('skills.deleteConfirm.title')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('skills.deleteConfirm.description', { name: deleteTarget?.name })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={isDeleting}>
              {t('skills.deleteConfirm.cancel')}
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={(e) => {
                e.preventDefault();
                handleConfirmDelete();
              }}
              disabled={isDeleting}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              {isDeleting ? t('common.loading') : t('skills.deleteConfirm.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
