// src/stores/skills-data.ts
import { create } from 'zustand';
import { toast } from 'sonner';
import {
  sortSkills,
  mergeUpdateInfo,
  updateInfoCache,
  clearUpdateCacheForContextSkill,
  type SkillListItem,
  type UpdateCheckDisplaySnapshot,
  t,
} from './skills-utils';
import {
  listSkills,
  checkUpdates,
  checkSkillAudit,
} from '@/hooks/useTauriApi';
import { toAppError } from '@/utils/to-app-error';
import { contextKey, globalContext } from '@/lib/context';
import type {
  AppError,
  ContextRef,
  ResolvedAgent,
  SkillScope,
  SkillUpdateInfo,
  SkillAuditData,
  SourceUpdateCheckInfo,
  UpdateCheckSelection,
  UpdateCheckResponse,
  UpdateCheckOutcome,
  UpdateResponse,
} from '@/bindings';

type UpdateCheckResult =
  | { ok: true; response: UpdateCheckResponse }
  | { ok: false };

type UpdateCheckPriority = 0 | 1;

interface UpdateCheckRequestTicket {
  key: string;
  generation: number;
  priority: UpdateCheckPriority;
}

const updateCheckRequestGenerations = new Map<string, number>();
const pendingUpdateCheckRequests = new Map<string, Set<number>>();
const admittedUpdateCheckRequests = new Map<string, UpdateCheckRequestTicket>();

function beginUpdateCheckRequest(
  key: string,
  priority: UpdateCheckPriority,
): UpdateCheckRequestTicket {
  const generation = (updateCheckRequestGenerations.get(key) ?? 0) + 1;
  updateCheckRequestGenerations.set(key, generation);
  const ticket = { key, generation, priority };
  const pending = pendingUpdateCheckRequests.get(key) ?? new Set<number>();
  pending.add(generation);
  pendingUpdateCheckRequests.set(key, pending);

  const admitted = admittedUpdateCheckRequests.get(key);
  if (!admitted || priority >= admitted.priority) {
    admittedUpdateCheckRequests.set(key, ticket);
  }
  return ticket;
}

function isAdmittedUpdateCheckRequest(ticket: UpdateCheckRequestTicket): boolean {
  return admittedUpdateCheckRequests.get(ticket.key)?.generation === ticket.generation;
}

function finishUpdateCheckRequest(ticket: UpdateCheckRequestTicket): boolean {
  const pending = pendingUpdateCheckRequests.get(ticket.key);
  pending?.delete(ticket.generation);
  if (pending && pending.size > 0) return true;

  pendingUpdateCheckRequests.delete(ticket.key);
  admittedUpdateCheckRequests.delete(ticket.key);
  return false;
}

function sourceUpdateIdentity(source: SourceUpdateCheckInfo): string {
  return `${source.source}\u0000${source.requestedRef ?? ''}`;
}

function mergeSourceUpdateInfo(
  previous: SourceUpdateCheckInfo[],
  next: SourceUpdateCheckInfo[],
): SourceUpdateCheckInfo[] {
  const nextKeys = new Set(next.map(sourceUpdateIdentity));
  return [
    ...previous.filter((source) => !nextKeys.has(sourceUpdateIdentity(source))),
    ...next,
  ];
}

function toUpdateCheckDisplaySnapshot(
  results: SkillUpdateInfo[],
  sources: SourceUpdateCheckInfo[],
  outcome: UpdateCheckOutcome,
  checkedAt: number,
): UpdateCheckDisplaySnapshot {
  return {
    outcome,
    sources,
    skillFreshness: Object.fromEntries(
      results.map((result) => [result.name, result.freshness]),
    ),
    checkedAt,
  };
}

function preserveLastConfirmedUpdates(
  previous: SkillUpdateInfo[],
  next: SkillUpdateInfo[],
): SkillUpdateInfo[] {
  const previousByName = new Map(previous.map((result) => [result.name, result]));
  return next.map((result) => {
    const last = previousByName.get(result.name);
    return result.status === 'cannotCheck' && last?.hasUpdate === true
      ? { ...result, hasUpdate: true }
      : result;
  });
}

function clearLocalUpdateFlags(
  skills: SkillListItem[],
  scope: SkillScope,
  skillNames: Set<string>,
  options: {
    clearCannotCheck?: boolean;
    clearCannotCheckNames?: ReadonlySet<string>;
  } = {},
): SkillListItem[] {
  let changed = false;
  const nextSkills = skills.map((skill) => {
    const shouldClear = skill.hasUpdate
      || options.clearCannotCheck
      || options.clearCannotCheckNames?.has(skill.name);
    if (skill.scope !== scope || !skillNames.has(skill.name) || !shouldClear) {
      return skill;
    }
    changed = true;
    return {
      ...skill,
      hasUpdate: false,
      updateStatus: 'upToDate' as const,
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
      response: await checkUpdates({
        context,
        mode: 'automatic',
        selection: { kind: 'all' },
      }),
    };
  } catch {
    return { ok: false };
  }
}

interface SkillsDataState {
  snapshots: Record<string, ContextSkillSnapshot>;
  auditCache: Record<string, SkillAuditData>;

  isSyncing: boolean;
  checkingUpdateScopes: Set<string>;

  // Actions
  refreshContext: (context: ContextRef) => Promise<void>;
  refreshWorkspace: (context: ContextRef) => Promise<void>;
  invalidateContexts: (contexts: ContextRef[]) => void;
  invalidateAgentProjections: () => void;
  syncSkills: (context: ContextRef) => Promise<void>;
  syncUpdates: (context: ContextRef) => Promise<void>;
  forceCheckUpdates: (
    context: ContextRef,
    selection: UpdateCheckSelection,
  ) => Promise<UpdateCheckOutcome | null>;
  applyUpdateResult: (context: ContextRef, response: UpdateResponse) => Promise<void>;
  fetchAuditForSkills: (skills: SkillListItem[]) => Promise<void>;
  markSourceRepairSucceeded: (context: ContextRef, skillName: string) => void;
}

export interface ContextSkillSnapshot {
  skills: SkillListItem[];
  updateCheck?: UpdateCheckDisplaySnapshot;
  agents: ResolvedAgent[];
  pathExists: boolean;
  loading: boolean;
  error: AppError | null;
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
  refreshContext: async (context) => {
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
      const result = await listSkills(context);
      const updateCache = updateInfoCache.get(key);
      const skills = sortSkills(
        updateCache
          ? mergeUpdateInfo(result.skills, updateCache.results, {
              preserveUnmatched: updateCache.completeness === 'partial',
              previousSkills: current.skills,
              sources: updateCache.sources,
            })
          : result.skills,
      );
      set((state) => {
        if (state.snapshots[key]?.requestId !== requestId) return {};
        return {
          snapshots: {
            ...state.snapshots,
            [key]: {
              skills,
              updateCheck: updateCache
                ? toUpdateCheckDisplaySnapshot(
                    updateCache.results,
                    updateCache.sources,
                    updateCache.outcome,
                    updateCache.checkedAt,
                  )
                : current.updateCheck,
              agents: result.agents,
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
              error: toAppError(error),
            },
          },
        };
      });
    }
  },

  refreshWorkspace: async (context) => {
    if (context.scope.scope === 'global') {
      await get().refreshContext(context);
      return;
    }
    await Promise.all([
      get().refreshContext(globalContext(context.environment)),
      get().refreshContext(context),
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

  invalidateAgentProjections: () => {
    set((state) => {
      for (const key of Object.keys(state.snapshots)) nextContextRequestGeneration(key);
      return { snapshots: {} };
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
    const key = contextKey(context);
    const ticket = beginUpdateCheckRequest(key, 0);
    set((state) => {
      const next = new Set(state.checkingUpdateScopes);
      next.add(key);
      return { checkingUpdateScopes: next };
    });
    try {
      const result = await checkUpdatesSafely(context);
      if (!result.ok || !isAdmittedUpdateCheckRequest(ticket)) return;
      const previous = updateInfoCache.get(key);
      const cacheEntry = {
        results: preserveLastConfirmedUpdates(previous?.results ?? [], result.response.skills),
        sources: result.response.sources,
        checkedAt: Date.now(),
        completeness: 'complete' as const,
        outcome: result.response.outcome,
      };
      updateInfoCache.set(key, cacheEntry);

      set((state) => {
        const current = state.snapshots[key] ?? emptyContextSnapshot();
        return {
          snapshots: {
            ...state.snapshots,
            [key]: {
              ...current,
              skills: sortSkills(mergeUpdateInfo(current.skills, cacheEntry.results, {
                sources: cacheEntry.sources,
              })),
              updateCheck: toUpdateCheckDisplaySnapshot(
                cacheEntry.results,
                cacheEntry.sources,
                cacheEntry.outcome,
                cacheEntry.checkedAt,
              ),
            },
          },
        };
      });
    } catch {
      // 静默失败 — 更新检测是非关键路径
    } finally {
      const stillPending = finishUpdateCheckRequest(ticket);
      set((state) => {
        if (stillPending) return {};
        const next = new Set(state.checkingUpdateScopes);
        next.delete(key);
        return { checkingUpdateScopes: next };
      });
    }
  },

  forceCheckUpdates: async (context, selection) => {
    const cacheKey = contextKey(context);
    const ticket = beginUpdateCheckRequest(cacheKey, 1);

    set((state) => {
      const next = new Set(state.checkingUpdateScopes);
      next.add(cacheKey);
      return { checkingUpdateScopes: next };
    });

    try {
      const response = await checkUpdates({
        context,
        mode: 'force',
        selection,
      });
      if (!isAdmittedUpdateCheckRequest(ticket)) return response.outcome;
      const now = Date.now();
      const previous = updateInfoCache.get(cacheKey);
      const updates = preserveLastConfirmedUpdates(previous?.results ?? [], response.skills);
      const results = selection.kind === 'skills'
        ? [
            ...(previous?.results ?? []).filter((item) => (
              !selection.skills.some((selected) => selected.skillName === item.name)
            )),
            ...updates,
          ]
        : updates;
      const sources = selection.kind === 'skills'
        ? mergeSourceUpdateInfo(previous?.sources ?? [], response.sources)
        : response.sources;
      const completeness = selection.kind === 'skills' ? 'partial' : 'complete';
      updateInfoCache.set(cacheKey, {
        results,
        sources,
        checkedAt: now,
        completeness,
        outcome: response.outcome,
      });
      set((state) => {
        const current = state.snapshots[cacheKey] ?? emptyContextSnapshot();
        return {
          snapshots: {
            ...state.snapshots,
            [cacheKey]: {
              ...current,
              skills: sortSkills(mergeUpdateInfo(current.skills, results, {
                preserveUnmatched: completeness === 'partial',
                sources,
              })),
              updateCheck: toUpdateCheckDisplaySnapshot(results, sources, response.outcome, now),
            },
          },
        };
      });
      return response.outcome;
    } catch (e) {
      toast.error(t('skills.checkUpdatesError', {
        error: e instanceof Error ? e.message : String(e),
      }));
      return null;
    } finally {
      const stillPending = finishUpdateCheckRequest(ticket);
      set((state) => {
        if (stillPending) return {};
        const next = new Set(state.checkingUpdateScopes);
        next.delete(cacheKey);
        return { checkingUpdateScopes: next };
      });
    }
  },

  applyUpdateResult: async (context, response) => {
    const snapshotKey = contextKey(context);
    const scope = context.scope.scope;
    const current = get().snapshots[snapshotKey] ?? emptyContextSnapshot();
    const successfulSkillNames = new Set(
      response.skills
        .filter((item) => item.mutation?.status === 'succeeded')
        .map((item) => item.skillIdentity.skillName),
    );
    const clearCannotCheckNames = new Set(
      current.skills
        .filter((skill) => successfulSkillNames.has(skill.name) && skill.updateReason === 'missingRemoteHash')
        .map((skill) => skill.name),
    );
    for (const skill of current.skills) {
      if (
        successfulSkillNames.has(skill.name)
        && (skill.canCheckForUpdates !== false || clearCannotCheckNames.has(skill.name))
      ) {
        clearUpdateCacheForContextSkill(skill.name, context, {
          clearCannotCheck: clearCannotCheckNames.has(skill.name),
        });
      }
    }
    if (successfulSkillNames.size > 0) {
      set((state) => {
        const snapshot = state.snapshots[snapshotKey] ?? emptyContextSnapshot();
        return {
          snapshots: {
            ...state.snapshots,
            [snapshotKey]: {
              ...snapshot,
              skills: clearLocalUpdateFlags(snapshot.skills, scope, successfulSkillNames, {
                clearCannotCheckNames,
              }),
            },
          },
        };
      });
    }
    await get().refreshContext(context);
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

}));
