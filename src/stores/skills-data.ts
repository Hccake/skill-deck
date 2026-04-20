// src/stores/skills-data.ts
import { create } from 'zustand';
import { toast } from 'sonner';
import { useContextStore } from './context';
import {
  sortSkills,
  mergeUpdateInfo,
  updateInfoCache,
  UPDATE_CHECK_TTL,
  clearUpdateCacheForSkill,
  type SkillListItem,
  t,
} from './skills-utils';
import {
  listSkills,
  listAgents,
  checkUpdates,
  updateSkill as apiUpdateSkill,
  updateSkillsBatch as apiUpdateSkillsBatch,
  checkSkillAudit,
} from '@/hooks/useTauriApi';
import { getSkillIdentity, getSkillIdentityKey, isSameSkillIdentity } from '@/lib/skills/identity';
import type { AgentInfo, InstalledSkill, SkillScope, SkillUpdateInfo, SkillAuditData } from '@/bindings';

type UpdateCheckResult =
  | { ok: true; updates: SkillUpdateInfo[] }
  | { ok: false };

function clearLocalUpdateFlags(
  skills: SkillListItem[],
  scope: SkillScope,
  skillNames: Set<string>,
): SkillListItem[] {
  let changed = false;
  const nextSkills = skills.map((skill) => {
    if (skill.scope !== scope || !skillNames.has(skill.name) || !skill.hasUpdate) {
      return skill;
    }
    changed = true;
    return {
      ...skill,
      hasUpdate: false,
      updateStatus: 'up-to-date' as const,
      updateReason: null,
    };
  });

  return changed ? nextSkills : skills;
}

async function checkUpdatesSafely(
  scope: SkillScope,
  projectPath?: string,
): Promise<UpdateCheckResult> {
  try {
    return {
      ok: true,
      updates: await checkUpdates(scope, projectPath),
    };
  } catch {
    return { ok: false };
  }
}

/** 内部共享加载逻辑 — fetchSkills 和 syncSkills 的唯一数据源 */
async function loadSkillsData(
  set: (partial: Partial<SkillsDataState> | ((state: SkillsDataState) => Partial<SkillsDataState>)) => void,
  options: { includeAgents: boolean },
) {
  const { selectedContext } = useContextStore.getState();
  const isProjectSelected = selectedContext !== 'global';

  if (isProjectSelected) {
    const [agents, globalResult, projectResult] = await Promise.all([
      options.includeAgents ? listAgents() : Promise.resolve(null),
      listSkills({ scope: 'global' }),
      listSkills({ scope: 'project', projectPath: selectedContext }),
    ]);

    const globalCache = updateInfoCache.get('global');
    const projectCache = updateInfoCache.get(selectedContext);
    const partial: Partial<SkillsDataState> = {
      globalSkills: sortSkills(
        globalCache ? mergeUpdateInfo(globalResult.skills, globalCache.results) : globalResult.skills
      ),
      projectSkills: sortSkills(
        projectCache ? mergeUpdateInfo(projectResult.skills, projectCache.results) : projectResult.skills
      ),
      projectPathExists: projectResult.pathExists,
    };
    if (agents) partial.allAgents = agents;
    set(partial);
  } else {
    const [agents, globalResult] = await Promise.all([
      options.includeAgents ? listAgents() : Promise.resolve(null),
      listSkills({ scope: 'global' }),
    ]);

    const globalCache = updateInfoCache.get('global');
    const partial: Partial<SkillsDataState> = {
      globalSkills: sortSkills(
        globalCache ? mergeUpdateInfo(globalResult.skills, globalCache.results) : globalResult.skills
      ),
      projectSkills: [],
      projectPathExists: true,
    };
    if (agents) partial.allAgents = agents;
    set(partial);
  }
}

interface SkillsDataState {
  // Data
  globalSkills: SkillListItem[];
  projectSkills: SkillListItem[];
  projectPathExists: boolean;
  /** Discover 页使用：所有已注册 project 的 skills（key = projectPath） */
  allProjectsSkills: Map<string, SkillListItem[]>;
  allAgents: AgentInfo[];
  loading: boolean;
  error: string | null;
  auditCache: Record<string, SkillAuditData>;

  // Operation state
  isSyncing: boolean;
  checkingUpdateScopes: Set<string>;
  updatingSkills: Map<string, 'queued' | 'updating' | 'done' | 'failed'>;
  updateAllCancelled: boolean;

  // Actions
  fetchSkills: () => Promise<void>;
  syncSkills: () => Promise<void>;
  syncUpdates: () => Promise<void>;
  forceCheckUpdates: (scope: SkillScope) => Promise<boolean>;
  fetchAuditForSkills: (skills: SkillListItem[]) => Promise<void>;
  updateSkill: (skillName: string, scope: SkillScope) => Promise<void>;
  updateAllInSection: (scope: SkillScope) => Promise<void>;
  cancelUpdateAll: () => void;
  /** 加载所有已注册 project 的 skills（供 Discover 页使用） */
  fetchAllProjectsSkills: () => Promise<void>;
}

export const useSkillsDataStore = create<SkillsDataState>()((set, get) => ({
  globalSkills: [],
  projectSkills: [],
  projectPathExists: true,
  allAgents: [],
  loading: true,
  error: null,
  auditCache: {},

  isSyncing: false,
  checkingUpdateScopes: new Set(),
  updatingSkills: new Map(),
  updateAllCancelled: false,
  allProjectsSkills: new Map(),

  fetchSkills: async () => {
    try {
      set({ loading: true, error: null });
      await loadSkillsData(set, { includeAgents: true });
    } catch (e) {
      set({ error: e instanceof Error ? e.message : 'Failed to load skills' });
    } finally {
      set({ loading: false });
    }
  },

  syncSkills: async () => {
    set({ isSyncing: true });
    try {
      await loadSkillsData(set, { includeAgents: false });
    } catch (e) {
      set({ error: e instanceof Error ? e.message : 'Failed to sync skills' });
    } finally {
      set({ isSyncing: false });
    }
  },

  syncUpdates: async () => {
    const contextAtStart = useContextStore.getState().selectedContext;
    const isProjectSelected = contextAtStart !== 'global';

    const now = Date.now();
    const globalCache = updateInfoCache.get('global');
    const projectCache = isProjectSelected ? updateInfoCache.get(contextAtStart) : null;
    const globalFresh = globalCache && (now - globalCache.checkedAt) < UPDATE_CHECK_TTL;
    const projectFresh = !isProjectSelected || (projectCache && (now - projectCache.checkedAt) < UPDATE_CHECK_TTL);
    if (globalFresh && projectFresh) return;

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
        const [globalResult, projectResult] = await Promise.all([
          globalFresh
            ? Promise.resolve({ ok: true, updates: globalCache!.results } satisfies UpdateCheckResult)
            : checkUpdatesSafely('global'),
          projectFresh
            ? Promise.resolve({ ok: true, updates: projectCache!.results } satisfies UpdateCheckResult)
            : checkUpdatesSafely('project', contextAtStart),
        ]);
        if (useContextStore.getState().selectedContext !== contextAtStart) return;
        if (!globalFresh && globalResult.ok) {
          updateInfoCache.set('global', { results: globalResult.updates, checkedAt: now });
        }
        if (!projectFresh && projectResult.ok) {
          updateInfoCache.set(contextAtStart, { results: projectResult.updates, checkedAt: now });
        }
        set((state) => ({
          globalSkills: globalResult.ok
            ? sortSkills(mergeUpdateInfo(state.globalSkills, globalResult.updates))
            : state.globalSkills,
          projectSkills: projectResult.ok
            ? sortSkills(mergeUpdateInfo(state.projectSkills, projectResult.updates))
            : state.projectSkills,
        }));
      } else {
        const globalResult = globalFresh
          ? ({ ok: true, updates: globalCache!.results } satisfies UpdateCheckResult)
          : await checkUpdatesSafely('global');
        if (useContextStore.getState().selectedContext !== contextAtStart) return;
        if (!globalFresh && globalResult.ok) {
          updateInfoCache.set('global', { results: globalResult.updates, checkedAt: now });
        }
        if (globalResult.ok) {
          set((state) => ({
            globalSkills: sortSkills(mergeUpdateInfo(state.globalSkills, globalResult.updates)),
          }));
        }
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

  forceCheckUpdates: async (scope) => {
    const { selectedContext } = useContextStore.getState();
    const isGlobal = scope === 'global';
    const contextAtStart = selectedContext;
    const cacheKey = isGlobal ? 'global' : contextAtStart;

    set((state) => {
      const next = new Set(state.checkingUpdateScopes);
      next.add(cacheKey);
      return { checkingUpdateScopes: next };
    });

    try {
      const projectPath = isGlobal ? undefined : contextAtStart;
      const updates = await checkUpdates(scope, projectPath);
      const now = Date.now();
      updateInfoCache.set(cacheKey, { results: updates, checkedAt: now });

      if (isGlobal) {
        set((state) => ({
          globalSkills: sortSkills(mergeUpdateInfo(state.globalSkills, updates)),
        }));
      } else if (useContextStore.getState().selectedContext === contextAtStart) {
        set((state) => ({
          projectSkills: sortSkills(mergeUpdateInfo(state.projectSkills, updates)),
        }));
      }
      return true;
    } catch (e) {
      toast.error(t('skills.checkUpdatesError', {
        error: e instanceof Error ? e.message : String(e),
      }));
      return false;
    } finally {
      set((state) => {
        const next = new Set(state.checkingUpdateScopes);
        next.delete(cacheKey);
        return { checkingUpdateScopes: next };
      });
    }
  },

  fetchAuditForSkills: async (skills) => {
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

  updateSkill: async (skillName, scope) => {
    const { updatingSkills } = get();
    const { selectedContext } = useContextStore.getState();
    const projectPath = scope === 'project' ? selectedContext : undefined;
    const skillIdentity = getSkillIdentity(
      { name: skillName, scope } as Pick<InstalledSkill, 'name' | 'scope'>,
      projectPath
    );
    const identityKey = getSkillIdentityKey(skillIdentity);
    if (updatingSkills.has(identityKey)) return;

    set((state) => {
      const next = new Map(state.updatingSkills);
      next.set(identityKey, 'updating');
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

      const shouldClearUpdateFlag = !item || item.status === 'success' || item.status === 'partial';
      if (shouldClearUpdateFlag) {
        clearUpdateCacheForSkill(skillName, scope, projectPath);
        set((state) => ({
          globalSkills: scope === 'global'
            ? clearLocalUpdateFlags(state.globalSkills, scope, new Set([skillName]))
            : state.globalSkills,
          projectSkills: scope === 'project'
            ? clearLocalUpdateFlags(state.projectSkills, scope, new Set([skillName]))
            : state.projectSkills,
        }));
      }

      set((state) => {
        const next = new Map(state.updatingSkills);
        next.set(identityKey, 'done');
        return { updatingSkills: next };
      });
      setTimeout(() => {
        set((state) => {
          const next = new Map(state.updatingSkills);
          next.delete(identityKey);
          return { updatingSkills: next };
        });
      }, 800);

      const { useSkillDetailStore } = await import('./skill-detail');

      // fire-and-forget: 不阻塞等待列表刷新 (async-defer-await)
      get().syncSkills().finally(() => {
        if (
          shouldClearUpdateFlag &&
          isSameSkillIdentity(useSkillDetailStore.getState().selectedSkillRef, skillIdentity)
        ) {
          void useSkillDetailStore.getState().reloadContent();
        }
      });
    } catch (e) {
      toast.error(t('skills.updateError', { name: skillName, error: e instanceof Error ? e.message : String(e) }));
      set((state) => {
        const next = new Map(state.updatingSkills);
        next.set(identityKey, 'failed');
        return { updatingSkills: next };
      });
      setTimeout(() => {
        set((state) => {
          const next = new Map(state.updatingSkills);
          next.delete(identityKey);
          return { updatingSkills: next };
        });
      }, 2000);
    }
  },

  updateAllInSection: async (scope) => {
    const { globalSkills, projectSkills } = get();
    const skills = scope === 'project' ? projectSkills : globalSkills;
    const updatable = skills.filter((s) => s.hasUpdate);
    if (updatable.length === 0) return;

    const { selectedContext } = useContextStore.getState();
    const projectPath = scope === 'project' ? selectedContext : undefined;

    set({ updateAllCancelled: false });
    set((state) => {
      const next = new Map(state.updatingSkills);
      for (const s of updatable) {
        next.set(getSkillIdentityKey({ name: s.name, scope: s.scope, projectPath }), 'queued');
      }
      return { updatingSkills: next };
    });

    const bySource = new Map<string, typeof updatable>();
    for (const skill of updatable) {
      const key = `${skill.sourceUrl ?? skill.source ?? '__no_source__'}::${skill.gitRef ?? ''}`;
      const group = bySource.get(key);
      if (group) {
        group.push(skill);
      } else {
        bySource.set(key, [skill]);
      }
    }

    const results: { name: string; success: boolean }[] = [];

    const groupPromises = Array.from(bySource.entries()).map(async ([, group]) => {
      if (get().updateAllCancelled) return;

      set((state) => {
        const next = new Map(state.updatingSkills);
        for (const s of group) {
          const identityKey = getSkillIdentityKey({ name: s.name, scope: s.scope, projectPath });
          if (next.get(identityKey) === 'queued') next.set(identityKey, 'updating');
        }
        return { updatingSkills: next };
      });

      try {
        const response = await apiUpdateSkillsBatch({
          scope,
          names: group.map((s) => s.name),
          projectPath,
        });
        const successfulSkillNames = new Set<string>();

        for (const skill of group) {
          const item = response.results.find((r) => r.name === skill.name);
          const success = !item || item.status === 'success' || item.status === 'partial';
          results.push({ name: skill.name, success });
          if (success) {
            successfulSkillNames.add(skill.name);
            clearUpdateCacheForSkill(skill.name, scope, projectPath);
          }
          set((state) => {
            const next = new Map(state.updatingSkills);
            next.set(
              getSkillIdentityKey({ name: skill.name, scope: skill.scope, projectPath }),
              success ? 'done' : 'failed'
            );
            return { updatingSkills: next };
          });
        }

        if (successfulSkillNames.size > 0) {
          set((state) => ({
            globalSkills: scope === 'global'
              ? clearLocalUpdateFlags(state.globalSkills, scope, successfulSkillNames)
              : state.globalSkills,
            projectSkills: scope === 'project'
              ? clearLocalUpdateFlags(state.projectSkills, scope, successfulSkillNames)
              : state.projectSkills,
          }));
        }
      } catch {
        for (const skill of group) {
          results.push({ name: skill.name, success: false });
          set((state) => {
            const next = new Map(state.updatingSkills);
            next.set(getSkillIdentityKey({ name: skill.name, scope: skill.scope, projectPath }), 'failed');
            return { updatingSkills: next };
          });
        }
      }
    });

    await Promise.all(groupPromises);

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

    // fire-and-forget (async-defer-await)
    get().syncSkills();
  },

  cancelUpdateAll: () => { set({ updateAllCancelled: true }); },

  fetchAllProjectsSkills: async () => {
    const { projects } = useContextStore.getState();
    if (projects.length === 0) {
      set({ allProjectsSkills: new Map() });
      return;
    }

    try {
      const results = await Promise.all(
        projects.map(async (projectPath) => {
          try {
            const result = await listSkills({ scope: 'project', projectPath });
            return [projectPath, result.skills] as const;
          } catch {
            return [projectPath, [] as InstalledSkill[]] as const;
          }
        })
      );
      set({ allProjectsSkills: new Map(results) });
    } catch {
      set({ allProjectsSkills: new Map() });
    }
  },
}));
