// src/stores/skills.ts
import { create } from 'zustand';
import { toast } from 'sonner';
import i18n from '@/i18n';
import { useContextStore } from './context';
import {
  listSkills,
  listAgents,
  removeSkill as apiRemoveSkill,
  getSkillAgentDetails as apiGetAgentDetails,
  checkUpdates,
  updateSkill as apiUpdateSkill,
  updateSkillsBatch as apiUpdateSkillsBatch,
  openInstallWizard,
  checkSkillAudit,
  readSkillContent as apiReadSkillContent,
  manageSkillAgents as apiManageSkillAgents,
  copySkillToProjects as apiCopySkillToProjects,
} from '@/hooks/useTauriApi';
import type { AgentInfo, AgentType, InstalledSkill, SkillScope, SkillUpdateInfo, SkillAuditData, SkillAgentDetails } from '@/bindings';

/** 按名称排序 skills，保证展示顺序稳定 */
function sortSkills(skills: InstalledSkill[]): InstalledSkill[] {
  return [...skills].sort((a, b) => a.name.localeCompare(b.name));
}

/** 将 check_updates 结果合并到 skills 列表 */
function mergeUpdateInfo(skills: InstalledSkill[], updates: SkillUpdateInfo[]): InstalledSkill[] {
  const updateMap = new Map(updates.map((u) => [u.name, u.hasUpdate]));
  return skills.map((s) => ({
    ...s,
    hasUpdate: updateMap.get(s.name) ?? false,
  }));
}

/** 更新检测结果的 scope 级缓存 — 避免频繁切换 scope 时重复网络请求 */
const updateInfoCache = new Map<string, { results: SkillUpdateInfo[]; checkedAt: number }>();
const UPDATE_CHECK_TTL = 5 * 60 * 1000; // 5 分钟

/** 清除缓存中指定 skill 的 hasUpdate 标记 — 更新成功后调用，防止 syncSkills 恢复旧标记 */
function clearUpdateCacheForSkill(skillName: string, scope: SkillScope) {
  const cacheKey = scope === 'project'
    ? useContextStore.getState().selectedContext
    : 'global';
  const cached = updateInfoCache.get(cacheKey);
  if (cached) {
    cached.results = cached.results.map((r) =>
      r.name === skillName ? { ...r, hasUpdate: false } : r
    );
  }
}

/** i18n t() 的便捷包装 */
function t(key: string, options?: Record<string, unknown>): string {
  return i18n.t(key, options);
}

export interface DeleteTarget {
  skill: InstalledSkill;
  scope: SkillScope;
  projectPath?: string;
}

export interface AddDialogPrefill {
  source: string;
  skillName: string;
}

interface SkillsState {
  // 数据层
  globalSkills: InstalledSkill[];
  projectSkills: InstalledSkill[];
  projectPathExists: boolean;
  allAgents: AgentInfo[];
  loading: boolean;
  error: string | null;

  // 审计数据缓存（key = skillName）
  auditCache: Record<string, SkillAuditData>;

  // 操作层
  isSyncing: boolean;
  /** 正在检测更新的 scope key 集合（'global' | 项目路径） */
  checkingUpdateScopes: Set<string>;
  updatingSkills: Map<string, 'queued' | 'updating' | 'done' | 'failed'>;
  updateAllCancelled: boolean;

  // Dialog 触发状态
  deleteTarget: DeleteTarget | null;
  agentDetails: SkillAgentDetails | null;
  loadingAgentDetails: boolean;

  // Detail panel state
  selectedSkill: InstalledSkill | null;
  skillContent: string | null;
  loadingContent: boolean;

  // Manage agents dialog state
  manageAgentsSkill: InstalledSkill | null;
  manageAgentsScope: SkillScope;

  // Copy to project dialog state
  copySkill: InstalledSkill | null;

  // Actions — 内部通过 useContextStore.getState() 获取 selectedContext
  fetchSkills: () => Promise<void>;
  syncSkills: () => Promise<void>;
  syncUpdates: () => Promise<void>;
  forceCheckUpdates: (scope: SkillScope) => Promise<void>;
  fetchAuditForSkills: (skills: InstalledSkill[]) => Promise<void>;
  updateSkill: (skillName: string, scope: SkillScope) => Promise<void>;
  updateAllInSection: (scope: SkillScope) => Promise<void>;
  cancelUpdateAll: () => void;
  deleteSkill: (params: { fullRemoval: boolean; agents?: AgentType[] }) => Promise<void>;
  selectSkill: (skill: InstalledSkill) => Promise<void>;
  deselectSkill: () => void;
  reloadContent: () => Promise<void>;
  openDelete: (skill: InstalledSkill, scope: SkillScope, projectPath?: string) => void;
  closeDelete: () => void;
  openAdd: (scope: SkillScope) => void;
  openAddWithPrefill: (prefill: AddDialogPrefill) => void;
  openManageAgents: (skill: InstalledSkill, scope: SkillScope) => void;
  closeManageAgents: () => void;
  saveAgentChanges: (addAgents: string[], removeAgents: string[]) => Promise<void>;
  openCopyToProject: (skill: InstalledSkill) => void;
  closeCopyToProject: () => void;
  executeCopy: (targetPaths: string[]) => Promise<void>;
}

export const useSkillsStore = create<SkillsState>()((set, get) => ({
  // 数据层初始值
  globalSkills: [],
  projectSkills: [],
  projectPathExists: true,
  allAgents: [],
  loading: true,
  error: null,

  // 审计缓存
  auditCache: {},

  // 操作层初始值
  isSyncing: false,
  checkingUpdateScopes: new Set(),
  updatingSkills: new Map(),
  updateAllCancelled: false,

  // Dialog 初始值
  deleteTarget: null,
  agentDetails: null,
  loadingAgentDetails: false,

  // Detail panel initial values
  selectedSkill: null,
  skillContent: null,
  loadingContent: false,

  // Manage agents dialog initial values
  manageAgentsSkill: null,
  manageAgentsScope: 'global' as SkillScope,

  // Copy to project dialog initial values
  copySkill: null,

  // === Actions ===

  fetchSkills: async () => {
    const { selectedContext } = useContextStore.getState();
    const isProjectSelected = selectedContext !== 'global';

    try {
      set({ loading: true, error: null });

      if (isProjectSelected) {
        const [agents, globalResult, projectResult] = await Promise.all([
          listAgents(),
          listSkills({ scope: 'global' }),
          listSkills({ scope: 'project', projectPath: selectedContext }),
        ]);
        // 立刻应用缓存的更新检测结果 — 切回 scope 时徽标瞬间出现
        const globalCache = updateInfoCache.get('global');
        const projectCache = updateInfoCache.get(selectedContext);
        set({
          allAgents: agents,
          globalSkills: sortSkills(
            globalCache ? mergeUpdateInfo(globalResult.skills, globalCache.results) : globalResult.skills
          ),
          projectSkills: sortSkills(
            projectCache ? mergeUpdateInfo(projectResult.skills, projectCache.results) : projectResult.skills
          ),
          projectPathExists: projectResult.pathExists,
        });
      } else {
        const [agents, globalResult] = await Promise.all([
          listAgents(),
          listSkills({ scope: 'global' }),
        ]);
        const globalCache = updateInfoCache.get('global');
        set({
          allAgents: agents,
          globalSkills: sortSkills(
            globalCache ? mergeUpdateInfo(globalResult.skills, globalCache.results) : globalResult.skills
          ),
          projectSkills: [],
          projectPathExists: true,
        });
      }
    } catch (e) {
      set({ error: e instanceof Error ? e.message : 'Failed to load skills' });
    } finally {
      set({ loading: false });
    }
  },

  fetchAuditForSkills: async (skills: InstalledSkill[]) => {
    // 按 source 分组，批量请求审计数据 — js-index-maps 规则
    const bySource = new Map<string, string[]>();
    for (const skill of skills) {
      if (!skill.source) continue;
      const existing = bySource.get(skill.source);
      if (existing) {
        existing.push(skill.name);
      } else {
        bySource.set(skill.source, [skill.name]);
      }
    }

    // 并行请求所有 source 的审计数据 — async-parallel 规则
    const results = await Promise.all(
      Array.from(bySource.entries()).map(([source, skillNames]) =>
        checkSkillAudit(source, skillNames).catch(() => null)
      )
    );

    const newCache: Record<string, SkillAuditData> = { ...get().auditCache };
    for (const result of results) {
      if (!result) continue;
      for (const [name, data] of Object.entries(result)) {
        if (data) newCache[name] = data;
      }
    }
    set({ auditCache: newCache });
  },

  syncSkills: async () => {
    const { selectedContext } = useContextStore.getState();
    const isProjectSelected = selectedContext !== 'global';

    set({ isSyncing: true });
    try {
      if (isProjectSelected) {
        const [globalResult, projectResult] = await Promise.all([
          listSkills({ scope: 'global' }),
          listSkills({ scope: 'project', projectPath: selectedContext }),
        ]);
        // 仅刷新列表，从缓存只读恢复 hasUpdate 标记
        const globalCache = updateInfoCache.get('global');
        const projectCache = updateInfoCache.get(selectedContext);
        set({
          globalSkills: sortSkills(
            globalCache ? mergeUpdateInfo(globalResult.skills, globalCache.results) : globalResult.skills
          ),
          projectSkills: sortSkills(
            projectCache ? mergeUpdateInfo(projectResult.skills, projectCache.results) : projectResult.skills
          ),
          projectPathExists: projectResult.pathExists,
        });
      } else {
        const globalResult = await listSkills({ scope: 'global' });
        const globalCache = updateInfoCache.get('global');
        set({
          globalSkills: sortSkills(
            globalCache ? mergeUpdateInfo(globalResult.skills, globalCache.results) : globalResult.skills
          ),
          projectSkills: [],
          projectPathExists: true,
        });
      }
    } catch (e) {
      set({ error: e instanceof Error ? e.message : 'Failed to sync skills' });
    } finally {
      set({ isSyncing: false });
    }
  },

  syncUpdates: async () => {
    const contextAtStart = useContextStore.getState().selectedContext;
    const isProjectSelected = contextAtStart !== 'global';

    // TTL 检查 — 缓存未过期则跳过网络请求
    const now = Date.now();
    const globalCache = updateInfoCache.get('global');
    const projectCache = isProjectSelected ? updateInfoCache.get(contextAtStart) : null;
    const globalFresh = globalCache && (now - globalCache.checkedAt) < UPDATE_CHECK_TTL;
    const projectFresh = !isProjectSelected || (projectCache && (now - projectCache.checkedAt) < UPDATE_CHECK_TTL);
    if (globalFresh && projectFresh) return; // 全部新鲜，跳过

    // 按 scope 独立设置 checking 状态
    const scopesToCheck: string[] = [];
    if (!globalFresh) scopesToCheck.push('global');
    if (!projectFresh) scopesToCheck.push(contextAtStart);
    set((state) => {
      const next = new Set(state.checkingUpdateScopes);
      for (const s of scopesToCheck) next.add(s);
      return { checkingUpdateScopes: next };
    });
    try {
      if (isProjectSelected) {
        // 仅请求过期的 scope
        const [globalUpdates, projectUpdates] = await Promise.all([
          globalFresh
            ? globalCache!.results
            : checkUpdates('global').catch(() => [] as SkillUpdateInfo[]),
          projectFresh
            ? projectCache!.results
            : checkUpdates('project', contextAtStart).catch(() => [] as SkillUpdateInfo[]),
        ]);
        if (useContextStore.getState().selectedContext !== contextAtStart) return;
        // 写入缓存
        if (!globalFresh) updateInfoCache.set('global', { results: globalUpdates, checkedAt: now });
        if (!projectFresh) updateInfoCache.set(contextAtStart, { results: projectUpdates, checkedAt: now });
        set((state) => ({
          globalSkills: sortSkills(mergeUpdateInfo(state.globalSkills, globalUpdates)),
          projectSkills: sortSkills(mergeUpdateInfo(state.projectSkills, projectUpdates)),
        }));
      } else {
        const globalUpdates = globalFresh
          ? globalCache!.results
          : await checkUpdates('global').catch(() => [] as SkillUpdateInfo[]);
        if (useContextStore.getState().selectedContext !== contextAtStart) return;
        if (!globalFresh) updateInfoCache.set('global', { results: globalUpdates, checkedAt: now });
        set((state) => ({
          globalSkills: sortSkills(mergeUpdateInfo(state.globalSkills, globalUpdates)),
        }));
      }
    } catch {
      // 静默失败 — 更新检测是非关键路径
    } finally {
      set((state) => {
        const next = new Set(state.checkingUpdateScopes);
        for (const s of scopesToCheck) next.delete(s);
        return { checkingUpdateScopes: next };
      });
    }
  },

  forceCheckUpdates: async (scope: SkillScope) => {
    const { selectedContext } = useContextStore.getState();
    const isGlobal = scope === 'global';
    const cacheKey = isGlobal ? 'global' : selectedContext;

    set((state) => {
      const next = new Set(state.checkingUpdateScopes);
      next.add(cacheKey);
      return { checkingUpdateScopes: next };
    });

    try {
      const projectPath = isGlobal ? undefined : selectedContext;
      const updates = await checkUpdates(scope, projectPath).catch(() => [] as SkillUpdateInfo[]);
      const now = Date.now();
      updateInfoCache.set(cacheKey, { results: updates, checkedAt: now });

      if (isGlobal) {
        set((state) => ({
          globalSkills: sortSkills(mergeUpdateInfo(state.globalSkills, updates)),
        }));
      } else {
        set((state) => ({
          projectSkills: sortSkills(mergeUpdateInfo(state.projectSkills, updates)),
        }));
      }
    } catch {
      // 静默失败
    } finally {
      set((state) => {
        const next = new Set(state.checkingUpdateScopes);
        next.delete(cacheKey);
        return { checkingUpdateScopes: next };
      });
    }
  },

  updateSkill: async (skillName: string, scope: SkillScope) => {
    const { updatingSkills } = get();
    if (updatingSkills.has(skillName)) return;

    const { selectedContext } = useContextStore.getState();
    const projectPath = scope === 'project' ? selectedContext : undefined;

    set((state) => {
      const next = new Map(state.updatingSkills);
      next.set(skillName, 'updating');
      return { updatingSkills: next };
    });

    try {
      const response = await apiUpdateSkill({ scope, name: skillName, projectPath });
      const item = response.results.find((r) => r.name === skillName) ?? response.results[0];
      const agentResults = item?.agentResults ?? [];
      const succeededAgents = agentResults.filter((r) => r.status === 'success').length;
      const failedAgents = agentResults.filter((r) => r.status === 'failed');
      const failedAgentNames = failedAgents.map((r) => r.agent).join(', ');

      if (!item || item.status === 'success') {
        toast.success(t('skills.updateSuccess', { name: skillName }));
      } else if (item.status === 'partial') {
        toast.warning(t('skills.updatePartial', { name: skillName, success: succeededAgents, total: agentResults.length, failed: failedAgents.length, failedAgents: failedAgentNames }));
      } else if (item.status === 'skipped') {
        toast.warning(t('skills.updateSkipped', { name: skillName }));
      } else {
        toast.error(t('skills.updateError', { name: skillName, error: item.error ?? t('skills.updateFailedUnknown') }));
      }

      if (item?.warnings?.length) {
        toast.warning(t('skills.updateWarning', { name: skillName, count: item.warnings.length, detail: item.warnings[0] }));
      }

      // Bug1 修复：清除缓存中的 hasUpdate 标记，防止 syncSkills 恢复旧标记
      clearUpdateCacheForSkill(skillName, scope);

      // Refresh detail panel content if this skill is selected
      const { selectedSkill } = get();
      if (selectedSkill?.name === skillName && selectedSkill?.scope === scope) {
        apiReadSkillContent(selectedSkill.canonicalPath)
          .then((content) => {
            if (get().selectedSkill?.name === skillName) {
              set({ skillContent: content });
            }
          })
          .catch(() => {}); // silent — update already succeeded
      }

      set((state) => {
        const next = new Map(state.updatingSkills);
        next.set(skillName, 'done');
        return { updatingSkills: next };
      });
      setTimeout(() => {
        set((state) => {
          const next = new Map(state.updatingSkills);
          next.delete(skillName);
          return { updatingSkills: next };
        });
      }, 800);

      await get().syncSkills();
    } catch (e) {
      toast.error(t('skills.updateError', { name: skillName, error: e instanceof Error ? e.message : String(e) }));
      set((state) => {
        const next = new Map(state.updatingSkills);
        next.set(skillName, 'failed');
        return { updatingSkills: next };
      });
      setTimeout(() => {
        set((state) => {
          const next = new Map(state.updatingSkills);
          next.delete(skillName);
          return { updatingSkills: next };
        });
      }, 2000);
    }
  },

  updateAllInSection: async (scope: SkillScope) => {
    const { globalSkills, projectSkills } = get();
    const skills = scope === 'project' ? projectSkills : globalSkills;
    const updatable = skills.filter((s) => s.hasUpdate);
    if (updatable.length === 0) return;

    set({ updateAllCancelled: false });
    set((state) => {
      const next = new Map(state.updatingSkills);
      for (const s of updatable) { next.set(s.name, 'queued'); }
      return { updatingSkills: next };
    });

    // 按 source 分组 — async-parallel 规则：不同 source 组并行
    const bySource = new Map<string, typeof updatable>();
    for (const skill of updatable) {
      const key = skill.source ?? '__no_source__';
      const group = bySource.get(key);
      if (group) {
        group.push(skill);
      } else {
        bySource.set(key, [skill]);
      }
    }

    const { selectedContext } = useContextStore.getState();
    const projectPath = scope === 'project' ? selectedContext : undefined;
    const results: { name: string; success: boolean }[] = [];

    // 并行调用各 source 组的 updateSkillsBatch
    const groupPromises = Array.from(bySource.entries()).map(async ([, group]) => {
      if (get().updateAllCancelled) return;

      // 标记该组全部为 updating
      set((state) => {
        const next = new Map(state.updatingSkills);
        for (const s of group) {
          if (next.get(s.name) === 'queued') next.set(s.name, 'updating');
        }
        return { updatingSkills: next };
      });

      try {
        const response = await apiUpdateSkillsBatch({
          scope,
          names: group.map((s) => s.name),
          projectPath,
        });

        for (const skill of group) {
          const item = response.results.find((r) => r.name === skill.name);
          const success = !item || item.status === 'success' || item.status === 'partial';
          results.push({ name: skill.name, success });
          if (success) clearUpdateCacheForSkill(skill.name, scope);
          set((state) => {
            const next = new Map(state.updatingSkills);
            next.set(skill.name, success ? 'done' : 'failed');
            return { updatingSkills: next };
          });
        }
      } catch {
        for (const skill of group) {
          results.push({ name: skill.name, success: false });
          set((state) => {
            const next = new Map(state.updatingSkills);
            next.set(skill.name, 'failed');
            return { updatingSkills: next };
          });
        }
      }
    });

    await Promise.all(groupPromises);

    // 取消时清理 queued 状态
    if (get().updateAllCancelled) {
      set((state) => {
        const next = new Map(state.updatingSkills);
        for (const [name, status] of next) {
          if (status === 'queued') next.delete(name);
        }
        return { updatingSkills: next };
      });
    }

    const succeeded = results.filter((r) => r.success).length;
    const failedItems = results.filter((r) => !r.success);
    const failedPart = failedItems.length > 0
      ? t('skills.updateAllFailed', { failed: failedItems.length, failedNames: failedItems.map((r) => r.name).join(', ') })
      : '';
    toast.info(t('skills.updateAllSummary', { total: results.length, succeeded, failedPart }));

    setTimeout(() => {
      set((state) => {
        const next = new Map(state.updatingSkills);
        for (const [name, status] of next) {
          if (status === 'done' || status === 'failed') next.delete(name);
        }
        return { updatingSkills: next };
      });
    }, 1500);

    await get().syncSkills();
  },

  cancelUpdateAll: () => { set({ updateAllCancelled: true }); },

  deleteSkill: async ({ fullRemoval, agents }) => {
    const { deleteTarget } = get();
    if (!deleteTarget) return;

    try {
      await apiRemoveSkill({
        scope: deleteTarget.scope,
        name: deleteTarget.skill.name,
        projectPath: deleteTarget.projectPath,
        fullRemoval,
        agents,
      });
      const msg = fullRemoval
        ? t('skills.deleteSuccess', { name: deleteTarget.skill.name })
        : t('skills.partialDeleteSuccess', { name: deleteTarget.skill.name, count: agents?.length ?? 0 });
      toast.success(msg);
      // Auto-deselect if the deleted skill was selected
      const { selectedSkill } = get();
      if (selectedSkill && selectedSkill.name === deleteTarget.skill.name) {
        get().deselectSkill();
      }
      set({ deleteTarget: null, agentDetails: null });
      await get().fetchSkills();
    } catch (e) {
      toast.error(t('skills.deleteError', {
        name: deleteTarget.skill.name,
        error: e instanceof Error ? e.message : String(e),
      }));
      set({ deleteTarget: null, agentDetails: null });
    }
  },

  // Detail panel actions
  selectSkill: async (skill) => {
    // js-early-exit: same skill → no-op (use reloadContent for retry)
    if (get().selectedSkill?.name === skill.name && get().selectedSkill?.scope === skill.scope) return;

    set({ selectedSkill: skill, skillContent: null, loadingContent: true });

    try {
      const content = await apiReadSkillContent(skill.canonicalPath);
      // Race condition guard: only apply if still the same skill
      if (get().selectedSkill?.name === skill.name && get().selectedSkill?.scope === skill.scope) {
        set({ skillContent: content, loadingContent: false });
      }
    } catch (e) {
      if (get().selectedSkill?.name === skill.name && get().selectedSkill?.scope === skill.scope) {
        console.error('[selectSkill] Failed to read content:', e);
        set({ skillContent: null, loadingContent: false });
      }
    }
  },

  deselectSkill: () => set({ selectedSkill: null, skillContent: null, loadingContent: false }),

  reloadContent: async () => {
    const { selectedSkill } = get();
    if (!selectedSkill) return;
    set({ skillContent: null, loadingContent: true });
    try {
      const content = await apiReadSkillContent(selectedSkill.canonicalPath);
      if (get().selectedSkill?.name === selectedSkill.name) {
        set({ skillContent: content, loadingContent: false });
      }
    } catch {
      if (get().selectedSkill?.name === selectedSkill.name) {
        set({ skillContent: null, loadingContent: false });
      }
    }
  },

  openDelete: (skill, scope, projectPath) => {
    // 立即打开对话框
    set({
      deleteTarget: { skill, scope, projectPath },
      agentDetails: null,
      loadingAgentDetails: true,
    });
    // fire-and-forget: prefetch agent 详情
    apiGetAgentDetails({ scope, name: skill.name, projectPath })
      .then((details) => set({ agentDetails: details }))
      .catch((e) => console.warn('Failed to fetch agent details:', e))
      .finally(() => set({ loadingAgentDetails: false }));
  },
  closeDelete: () => set({ deleteTarget: null, agentDetails: null, loadingAgentDetails: false }),

  openAdd: (scope) => {
    const { selectedContext } = useContextStore.getState();
    openInstallWizard({
      entryPoint: 'skills-panel',
      scope,
      projectPath: scope === 'project' ? selectedContext : undefined,
    }).catch((e) => {
      console.error('[openAdd] Failed to open wizard:', e);
      toast.error(String(e));
    });
  },
  openAddWithPrefill: (prefill) => {
    openInstallWizard({
      entryPoint: 'discovery',
      scope: 'global',
      prefillSource: prefill.source,
      prefillSkillName: prefill.skillName,
    }).catch((e) => {
      console.error('[openAddWithPrefill] Failed to open wizard:', e);
      toast.error(String(e));
    });
  },

  openManageAgents: (skill, scope) => {
    set({ manageAgentsSkill: skill, manageAgentsScope: scope });
  },
  closeManageAgents: () => set({ manageAgentsSkill: null }),
  saveAgentChanges: async (addAgents, removeAgents) => {
    const { manageAgentsSkill, manageAgentsScope } = get();
    if (!manageAgentsSkill) return;

    const { selectedContext } = useContextStore.getState();
    const scope = manageAgentsScope === 'project' ? 'project' : 'global';
    const projectPath = scope === 'project' ? selectedContext : undefined;

    try {
      const result = await apiManageSkillAgents({
        skillName: manageAgentsSkill.name,
        scope,
        projectPath,
        addAgents: addAgents as AgentType[],
        removeAgents: removeAgents as AgentType[],
      });

      if (result.errors.length > 0) {
        toast.error(result.errors.join('\n'));
      } else {
        toast.success(t('skills.manageAgents.success'));
      }

      set({ manageAgentsSkill: null });
      // 刷新 skills 列表
      await get().syncSkills();
      // 刷新 selectedSkill（agents 列表已变化）
      const { selectedSkill, globalSkills, projectSkills } = get();
      if (selectedSkill) {
        const allSkills = [...globalSkills, ...projectSkills];
        const updated = allSkills.find((s) => s.name === selectedSkill.name && s.scope === selectedSkill.scope);
        if (updated) {
          set({ selectedSkill: updated });
        }
      }
    } catch (e) {
      console.error('[saveAgentChanges] Failed:', e);
      toast.error(String(e));
    }
  },

  openCopyToProject: (skill) => {
    set({ copySkill: skill });
  },
  closeCopyToProject: () => set({ copySkill: null }),
  executeCopy: async (targetPaths) => {
    const { copySkill } = get();
    if (!copySkill) return;

    const { selectedContext } = useContextStore.getState();

    try {
      const result = await apiCopySkillToProjects({
        skillName: copySkill.name,
        sourceProjectPath: selectedContext,
        targetProjectPaths: targetPaths,
        agents: copySkill.agents,
      });

      const successCount = result.results.filter((r) => r.success).length;
      const failCount = result.results.filter((r) => !r.success).length;

      if (failCount > 0) {
        const errors = result.results
          .filter((r) => !r.success)
          .map((r) => `${r.projectPath}: ${r.error}`)
          .join('\n');
        toast.error(t('skills.copyToProject.partialError', { success: successCount, fail: failCount }) + '\n' + errors);
      } else {
        toast.success(t('skills.copyToProject.success', { count: successCount }));
      }

      set({ copySkill: null });
    } catch (e) {
      console.error('[executeCopy] Failed:', e);
      toast.error(String(e));
    }
  },
}));
