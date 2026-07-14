// src/stores/skills-data.ts
import { create } from 'zustand';
import { toast } from 'sonner';
import { isMutationWriteBlocked } from './mutation';
import { useProjectStore } from './projects';
import {
  sortSkills,
  mergeUpdateInfo,
  updateInfoCache,
  UPDATE_CHECK_TTL,
  clearUpdateCacheForContextSkill,
  buildUpdatePlan,
  type SkillListItem,
  type UpdatePlan,
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
import { appendCrossStorageFailureGuidance } from '@/utils/cross-storage-guidance';
import { contextKey, environmentKey, globalContext } from '@/lib/context';
import type {
  AgentInfo,
  ContextRef,
  InstalledSkill,
  SkillScope,
  SkillUpdateInfo,
  SkillAuditData,
  UpdateSkillItemResult,
} from '@/bindings';

type UpdateCheckResult =
  | { ok: true; updates: SkillUpdateInfo[] }
  | { ok: false };

function clearLocalUpdateFlags(
  skills: SkillListItem[],
  scope: SkillScope,
  skillNames: Set<string>,
  options: { clearCannotCheck?: boolean } = {},
): SkillListItem[] {
  let changed = false;
  const nextSkills = skills.map((skill) => {
    const shouldClear = skill.hasUpdate || options.clearCannotCheck;
    if (skill.scope !== scope || !skillNames.has(skill.name) || !shouldClear) {
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
  context: ContextRef,
): Promise<UpdateCheckResult> {
  try {
    return {
      ok: true,
      updates: await checkUpdates(context),
    };
  } catch {
    return { ok: false };
  }
}

function projectPathForContext(context: ContextRef): string | undefined {
  const scope = context.scope;
  if (scope.scope !== 'project') return undefined;
  const projects = useProjectStore.getState().projectsByEnvironment[
    environmentKey(context.environment)
  ] ?? [];
  return projects.find((project) => project.binding.id === scope.project_id)
    ?.binding.nativePath;
}

interface SkillsDataState {
  snapshots: Record<string, ContextSkillSnapshot>;
  auditCache: Record<string, SkillAuditData>;

  // Operation state
  isSyncing: boolean;
  checkingUpdateScopes: Set<string>;
  updatingSkills: Map<string, 'updating' | 'done' | 'failed'>;
  lastUpdatePlan: UpdatePlan | null;
  lastUpdateResults: UpdateSkillItemResult[] | null;
  lastFailedUpdateNames: string[];

  // Actions
  refreshContext: (context: ContextRef, includeAgents?: boolean) => Promise<void>;
  refreshWorkspace: (context: ContextRef) => Promise<void>;
  invalidateContexts: (contexts: ContextRef[]) => void;
  syncSkills: (context: ContextRef) => Promise<void>;
  syncUpdates: (context: ContextRef) => Promise<void>;
  forceCheckUpdates: (context: ContextRef) => Promise<boolean>;
  fetchAuditForSkills: (skills: SkillListItem[]) => Promise<void>;
  updateSkill: (context: ContextRef, skillName: string) => Promise<void>;
  markSourceRepairSucceeded: (context: ContextRef, skillName: string) => void;
  updateAllInSection: (context: ContextRef) => Promise<void>;
}

export interface ContextSkillSnapshot {
  skills: SkillListItem[];
  agents: AgentInfo[];
  pathExists: boolean;
  loading: boolean;
  error: string | null;
  requestId: number;
}

function emptyContextSnapshot(): ContextSkillSnapshot {
  return {
    skills: [],
    agents: [],
    pathExists: true,
    loading: false,
    error: null,
    requestId: 0,
  };
}

const contextRequestGenerations = new Map<string, number>();

function nextContextRequestGeneration(key: string): number {
  const generation = (contextRequestGenerations.get(key) ?? 0) + 1;
  contextRequestGenerations.set(key, generation);
  return generation;
}

export const useSkillsDataStore = create<SkillsDataState>()((set, get) => ({
  snapshots: {},
  auditCache: {},

  isSyncing: false,
  checkingUpdateScopes: new Set(),
  updatingSkills: new Map(),
  lastUpdatePlan: null,
  lastUpdateResults: null,
  lastFailedUpdateNames: [],
  refreshContext: async (context, includeAgents = true) => {
    const key = contextKey(context);
    const current = get().snapshots[key] ?? emptyContextSnapshot();
    const requestId = nextContextRequestGeneration(key);
    set((state) => ({
      snapshots: {
        ...state.snapshots,
        [key]: {
          ...(state.snapshots[key] ?? emptyContextSnapshot()),
          loading: true,
          error: null,
          requestId,
        },
      },
    }));

    try {
      const [result, agents] = await Promise.all([
        listSkills(context),
        includeAgents
          ? listAgents(context)
          : Promise.resolve(current.agents),
      ]);
      const updateCache = updateInfoCache.get(key);
      const skills = sortSkills(
        updateCache ? mergeUpdateInfo(result.skills, updateCache.results) : result.skills,
      );
      set((state) => {
        if (state.snapshots[key]?.requestId !== requestId) return {};
        return {
          snapshots: {
            ...state.snapshots,
            [key]: {
              skills,
              agents,
              pathExists: result.pathExists,
              loading: false,
              error: null,
              requestId,
            },
          },
        };
      });
    } catch (error) {
      set((state) => {
        if (state.snapshots[key]?.requestId !== requestId) return {};
        return {
          snapshots: {
            ...state.snapshots,
            [key]: {
              ...state.snapshots[key],
              loading: false,
              error: error instanceof Error ? error.message : String(error),
            },
          },
        };
      });
    }
  },

  refreshWorkspace: async (context) => {
    if (context.scope.scope === 'global') {
      await get().refreshContext(context, true);
      return;
    }
    await Promise.all([
      get().refreshContext(globalContext(context.environment), false),
      get().refreshContext(context, true),
    ]);
  },

  invalidateContexts: (contexts) => {
    set((state) => {
      const keys = new Set(contexts.map(contextKey));
      for (const key of keys) nextContextRequestGeneration(key);
      return {
        snapshots: Object.fromEntries(
          Object.entries(state.snapshots).filter(([key]) => !keys.has(key)),
        ),
      };
    });
  },

  syncSkills: async (context) => {
    set({ isSyncing: true });
    try {
      await get().refreshWorkspace(context);
    } finally {
      set({ isSyncing: false });
    }
  },

  syncUpdates: async (context) => {
    const contexts = context.scope.scope === 'project'
      ? [globalContext(context.environment), context]
      : [context];
    const now = Date.now();
    const contextsToCheck = contexts.filter((candidate) => {
      const cached = updateInfoCache.get(contextKey(candidate));
      return !cached || now - cached.checkedAt >= UPDATE_CHECK_TTL;
    });
    const keysToCheck = contextsToCheck.map(contextKey);
    set((state) => {
      const next = new Set(state.checkingUpdateScopes);
      for (const key of keysToCheck) next.add(key);
      return { checkingUpdateScopes: next };
    });
    try {
      const checkedResults = await Promise.all(
        contextsToCheck.map(async (candidate) => ({
          context: candidate,
          result: await checkUpdatesSafely(candidate),
        })),
      );
      for (const { context: checkedContext, result } of checkedResults) {
        if (result.ok) {
          updateInfoCache.set(contextKey(checkedContext), {
            results: result.updates,
            checkedAt: now,
          });
        }
      }

      set((state) => {
        const snapshots = { ...state.snapshots };
        for (const candidate of contexts) {
          const key = contextKey(candidate);
          const cached = updateInfoCache.get(key);
          if (!cached) continue;
          const current = snapshots[key] ?? emptyContextSnapshot();
          snapshots[key] = {
            ...current,
            skills: sortSkills(mergeUpdateInfo(current.skills, cached.results)),
          };
        }
        return { snapshots };
      });
    } catch {
      // 静默失败 — 更新检测是非关键路径
    } finally {
      set((state) => {
        const next = new Set(state.checkingUpdateScopes);
        for (const key of keysToCheck) next.delete(key);
        return { checkingUpdateScopes: next };
      });
    }
  },

  forceCheckUpdates: async (context) => {
    const cacheKey = contextKey(context);

    set((state) => {
      const next = new Set(state.checkingUpdateScopes);
      next.add(cacheKey);
      return { checkingUpdateScopes: next };
    });

    try {
      const updates = await checkUpdates(context);
      const now = Date.now();
      updateInfoCache.set(cacheKey, { results: updates, checkedAt: now });
      set((state) => {
        const current = state.snapshots[cacheKey] ?? emptyContextSnapshot();
        return {
          snapshots: {
            ...state.snapshots,
            [cacheKey]: {
              ...current,
              skills: sortSkills(mergeUpdateInfo(current.skills, updates)),
            },
          },
        };
      });
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

  updateSkill: async (context, skillName) => {
    if (isMutationWriteBlocked()) return;
    const { updatingSkills } = get();
    const scope = context.scope.scope;
    const projectPath = projectPathForContext(context);
    const snapshotKey = contextKey(context);
    const skillIdentity = getSkillIdentity(
      { name: skillName, scope } as Pick<InstalledSkill, 'name' | 'scope'>,
      projectPath
    );
    const identityKey = getSkillIdentityKey(skillIdentity);
    if (updatingSkills.has(identityKey)) return;

    const skillsList = get().snapshots[snapshotKey]?.skills ?? [];
    const target = skillsList.find((s) => s.name === skillName);
    if (target?.updateStatus === 'deleted-upstream' || target?.updateReason === 'deleted-upstream') {
      toast.info(t('skills.updatePlan.deletedUpstreamDescription'));
      return;
    }

    set((state) => {
      const next = new Map(state.updatingSkills);
      next.set(identityKey, 'updating');
      return { updatingSkills: next };
    });

    try {
      const response = await apiUpdateSkill(context, skillName);
      const item = response.results.find((r) => r.name === skillName) ?? response.results[0];
      const agentResults = item?.agentResults ?? [];
      const succeededAgents = agentResults.filter((r) => r.status === 'success').length;
      const failedAgents = agentResults.filter((r) => r.status === 'failed');
      const failedAgentNames = failedAgents.map((r) => r.agent).join(', ');

      if (!item || item.status === 'success') {
        toast.success(t('skills.updateSuccess', { name: skillName }));
      } else if (item.status === 'partial') {
        toast.warning(appendCrossStorageFailureGuidance(
          t('skills.updatePartial', { name: skillName, success: succeededAgents, total: agentResults.length, failed: failedAgents.length, failedAgents: failedAgentNames }),
          context,
          'update',
          t,
        ));
      } else if (item.status === 'skipped') {
        toast.warning(t('skills.updateSkipped', { name: skillName }));
      } else {
        toast.error(appendCrossStorageFailureGuidance(
          t('skills.updateError', {
            name: skillName,
            error: item.error ?? t('skills.updateFailedUnknown'),
          }),
          context,
          'update',
          t,
        ));
      }

      if (item?.warnings?.length) {
        toast.warning(t('skills.updateWarning', { name: skillName, count: item.warnings.length, detail: item.warnings[0] }));
      }

      // Partial / Failed 不清缓存:后端在这两个状态下不会更新 lock,
      // 保留 hasUpdate 让用户能再次点击重试,避免失败信息被吞掉。
      const shouldClearUpdateFlag = !item || item.status === 'success';
      if (shouldClearUpdateFlag) {
        const shouldClearCannotCheck = target?.updateReason === 'missing-remote-hash';
        if (target?.canCheckForUpdates !== false || shouldClearCannotCheck) {
          clearUpdateCacheForContextSkill(skillName, context, {
            clearCannotCheck: shouldClearCannotCheck,
          });
        }
        set((state) => {
          const current = state.snapshots[snapshotKey] ?? emptyContextSnapshot();
          return {
            snapshots: {
              ...state.snapshots,
              [snapshotKey]: {
                ...current,
                skills: clearLocalUpdateFlags(
                  current.skills,
                  scope,
                  new Set([skillName]),
                  { clearCannotCheck: shouldClearCannotCheck },
                ),
              },
            },
          };
        });
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
      get().syncSkills(context).finally(() => {
        if (
          shouldClearUpdateFlag &&
          isSameSkillIdentity(useSkillDetailStore.getState().selectedSkillRef, skillIdentity)
        ) {
          void useSkillDetailStore.getState().reloadContent();
        }
      });
    } catch (e) {
      toast.error(appendCrossStorageFailureGuidance(
        t('skills.updateError', {
          name: skillName,
          error: e instanceof Error ? e.message : String(e),
        }),
        context,
        'update',
        t,
      ));
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

  markSourceRepairSucceeded: (context, skillName) => {
    clearUpdateCacheForContextSkill(skillName, context, { clearCannotCheck: true });
    const key = contextKey(context);
    const scope = context.scope.scope;
    set((state) => {
      const current = state.snapshots[key] ?? emptyContextSnapshot();
      return {
        snapshots: {
          ...state.snapshots,
          [key]: {
            ...current,
            skills: clearLocalUpdateFlags(
              current.skills,
              scope,
              new Set([skillName]),
              { clearCannotCheck: true },
            ),
          },
        },
      };
    });
  },

  updateAllInSection: async (context) => {
    if (isMutationWriteBlocked()) return;
    const scope = context.scope.scope;
    const snapshotKey = contextKey(context);
    const skills = get().snapshots[snapshotKey]?.skills ?? [];
    const projectPath = projectPathForContext(context);
    const plan = buildUpdatePlan(skills, scope, projectPath);
    const updatableNames = new Set(plan.groups.flatMap((group) => group.skillNames));
    const updatable = skills.filter((s) => updatableNames.has(s.name));

    set({ lastUpdatePlan: plan, lastUpdateResults: null, lastFailedUpdateNames: [] });
    if (updatable.length === 0) return;

    set((state) => {
      const next = new Map(state.updatingSkills);
      for (const s of updatable) {
        next.set(getSkillIdentityKey({ name: s.name, scope: s.scope, projectPath }), 'updating');
      }
      return { updatingSkills: next };
    });

    let itemResults: UpdateSkillItemResult[];
    try {
      const response = await apiUpdateSkillsBatch(
        context,
        updatable.map((skill) => skill.name),
      );
      const responseByName = new Map(response.results.map((item) => [item.name, item]));
      itemResults = updatable.map((skill) => {
        const item = responseByName.get(skill.name) ?? {
          name: skill.name,
          status: 'failed' as const,
          error: t('skills.updateFailedUnknown'),
          warnings: [],
          agentResults: [],
        };
        if (item.status !== 'failed' && item.status !== 'partial') return item;
        return {
          ...item,
          error: appendCrossStorageFailureGuidance(
            item.error ?? t('skills.updateFailedUnknown'),
            context,
            'update',
            t,
          ),
        };
      });
    } catch {
      itemResults = updatable.map((skill) => ({
        name: skill.name,
        status: 'failed',
        error: appendCrossStorageFailureGuidance(
          t('skills.updateFailedUnknown'),
          context,
          'update',
          t,
        ),
        warnings: [],
        agentResults: [],
      }));
    }

    const resultByName = new Map(itemResults.map((item) => [item.name, item]));
    const successfulSkillNames = new Set(
      itemResults.filter((item) => item.status === 'success').map((item) => item.name),
    );
    for (const skill of updatable) {
      if (successfulSkillNames.has(skill.name) && skill.canCheckForUpdates !== false) {
        clearUpdateCacheForContextSkill(skill.name, context);
      }
    }
    set((state) => {
      const nextUpdating = new Map(state.updatingSkills);
      for (const skill of updatable) {
        const item = resultByName.get(skill.name);
        nextUpdating.set(
          getSkillIdentityKey({ name: skill.name, scope: skill.scope, projectPath }),
          item?.status === 'failed' ? 'failed' : 'done',
        );
      }
      const current = state.snapshots[snapshotKey] ?? emptyContextSnapshot();
      return {
        updatingSkills: nextUpdating,
        snapshots: successfulSkillNames.size === 0 ? state.snapshots : {
          ...state.snapshots,
          [snapshotKey]: {
            ...current,
            skills: clearLocalUpdateFlags(current.skills, scope, successfulSkillNames),
          },
        },
      };
    });

    const failedItems = itemResults.filter((item) => item.status === 'failed');
    const succeeded = itemResults.length - failedItems.length;
    set({
      lastUpdateResults: itemResults,
      lastFailedUpdateNames: itemResults
        .filter((item) => item.status === 'failed' || item.status === 'partial')
        .map((item) => item.name),
    });
    const failedPart = failedItems.length > 0
      ? t('skills.updateAllFailed', { failed: failedItems.length, failedNames: failedItems.map((item) => item.name).join(', ') })
      : '';
    toast.info(t('skills.updateAllSummary', { total: itemResults.length, succeeded, failedPart }));

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
    get().syncSkills(context);
  },

}));
