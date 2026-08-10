// src/stores/skills-data.ts
import { create } from 'zustand';
import { toast } from 'sonner';
import {
  sortSkills,
  mergeUpdateInfo,
  updateInfoCache,
  clearUpdateCacheForContextSkill,
  normalizeSourceIdentity,
  type SkillListItem,
  type UpdateCheckDisplaySnapshot,
  t,
} from './skills-utils';
import {
  listSkills,
  checkUpdates,
} from '@/hooks/useTauriApi';
import { toAppError } from '@/utils/to-app-error';
import { contextKey, environmentKey, globalContext } from '@/lib/context';
import type {
  AppError,
  SkillLocationRef,
  ResolvedAgent,
  InstalledSkillLocation,
  SkillUpdateInfo,
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

export type RefreshOrigin = 'initial' | 'passive' | 'selfMutation';

export type RefreshOptions =
  | { origin?: Exclude<RefreshOrigin, 'selfMutation'>; mutatedSkillNames?: never }
  | { origin: 'selfMutation'; mutatedSkillNames: string[] };

export interface UpdateCheckSession {
  active: boolean;
  initialAttempted: boolean;
  observedSkills: Record<string, string>;
  automaticPending: boolean;
  forcePending: boolean;
}

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
  return `${normalizeSourceIdentity(source.source)}\u0000${source.requestedRef ?? 'HEAD'}`;
}

function sourceMatchesSkillUpdate(
  source: SourceUpdateCheckInfo,
  update: SkillUpdateInfo,
): boolean {
  return normalizeSourceIdentity(source.source)
      === normalizeSourceIdentity(update.sourceUrl ?? update.source)
    && (source.requestedRef ?? 'HEAD') === (update.gitRef ?? 'HEAD');
}

function emptyUpdateCheckSession(): UpdateCheckSession {
  return {
    active: false,
    initialAttempted: false,
    observedSkills: {},
    automaticPending: false,
    forcePending: false,
  };
}

function skillCheckFingerprint(context: SkillLocationRef, skill: SkillListItem): string {
  return [
    contextKey(context),
    skill.name,
    skill.source ?? '',
    skill.gitRef ?? '',
    skill.skillPath ?? '',
  ].join('\u0000');
}

function eligibleSkills(snapshot: ContextSkillSnapshot): SkillListItem[] {
  return snapshot.skills.filter((skill) => skill.canCheckForUpdates === true);
}

function pendingScopes(sessions: Record<string, UpdateCheckSession>): {
  checking: Set<string>;
  automatic: Set<string>;
  force: Set<string>;
} {
  const checking = new Set<string>();
  const automatic = new Set<string>();
  const force = new Set<string>();
  for (const [key, session] of Object.entries(sessions)) {
    if (session.automaticPending) automatic.add(key);
    if (session.forcePending) force.add(key);
    if (session.automaticPending || session.forcePending) checking.add(key);
  }
  return { checking, automatic, force };
}

function mergeSourceUpdateInfo(
  previous: SourceUpdateCheckInfo[],
  next: SourceUpdateCheckInfo[],
  retainedResults: SkillUpdateInfo[],
): SourceUpdateCheckInfo[] {
  const nextKeys = new Set(next.map(sourceUpdateIdentity));
  return [
    ...previous.filter((source) => {
      const identity = sourceUpdateIdentity(source);
      return retainedResults.some((update) => sourceMatchesSkillUpdate(source, update))
        && !nextKeys.has(identity);
    }),
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
  const identityKey = (result: SkillUpdateInfo) => [
    result.name,
    result.sourceUrl ?? result.source ?? '',
    result.gitRef ?? '',
    result.skillPath ?? '',
  ].join('\u0000');
  const previousByIdentity = new Map(previous.map((result) => [identityKey(result), result]));
  return next.map((result) => {
    const last = previousByIdentity.get(identityKey(result));
    const hasCommittedComparison = last?.status === 'upToDate'
      || last?.status === 'updateAvailable'
      || last?.status === 'deletedUpstream';
    return result.status === 'cannotCheck'
      && result.reason === 'upstreamUnavailable'
      && hasCommittedComparison
      ? {
          ...result,
          hasUpdate: last.hasUpdate,
          status: last.status,
          reason: last.reason,
        }
      : result;
  });
}

function clearLocalUpdateFlags(
  skills: SkillListItem[],
  scope: InstalledSkillLocation,
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

interface SkillsDataState {
  snapshots: Record<string, ContextSkillSnapshot>;
  updateCheckSessions: Record<string, UpdateCheckSession>;

  isSyncing: boolean;
  checkingUpdateScopes: Set<string>;
  automaticUpdateScopes: Set<string>;
  forceUpdateScopes: Set<string>;

  // Actions
  refreshContext: (context: SkillLocationRef, options?: RefreshOptions) => Promise<void>;
  refreshWorkspace: (context: SkillLocationRef, options?: RefreshOptions) => Promise<void>;
  invalidateContexts: (contexts: SkillLocationRef[]) => void;
  invalidateAgentProjections: () => void;
  syncSkills: (context: SkillLocationRef, options?: RefreshOptions) => Promise<void>;
  syncUpdates: (context: SkillLocationRef) => Promise<void>;
  activateAutomaticChecks: (context: SkillLocationRef) => Promise<void>;
  reconcileAutomaticChecks: (context: SkillLocationRef) => Promise<void>;
  forceCheckUpdates: (
    context: SkillLocationRef,
    selection: UpdateCheckSelection,
  ) => Promise<UpdateCheckOutcome | null>;
  applyUpdateResult: (context: SkillLocationRef, response: UpdateResponse) => Promise<void>;
  markSourceRepairSucceeded: (context: SkillLocationRef, skillName: string) => void;
  clearNativeGithubProviderCooldown: () => void;
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

export function sourceDiagnosticsForEnvironment(
  snapshots: Record<string, ContextSkillSnapshot>,
  environment: SkillLocationRef['environment'],
): SourceUpdateCheckInfo[] {
  const prefix = `${environmentKey(environment)}/`;
  return Object.entries(snapshots).flatMap(([key, snapshot]) => (
    key.startsWith(prefix) ? snapshot.updateCheck?.sources ?? [] : []
  ));
}

function clearNativeGithubProviderCooldown(
  source: SourceUpdateCheckInfo,
): SourceUpdateCheckInfo {
  const failure = source.lastAttempt?.failure;
  const identity = normalizeSourceIdentity(source.source);
  if (
    !failure?.providerCooldown
    || !(identity === 'github.com' || identity?.startsWith('github.com/'))
  ) {
    return source;
  }
  return {
    ...source,
    lastAttempt: {
      ...source.lastAttempt!,
      failure: {
        ...failure,
        retryAtEpochMs: null,
        providerCooldown: false,
      },
    },
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
  updateCheckSessions: {},

  isSyncing: false,
  checkingUpdateScopes: new Set(),
  automaticUpdateScopes: new Set(),
  forceUpdateScopes: new Set(),
  clearNativeGithubProviderCooldown: () => {
    for (const [key, cacheEntry] of updateInfoCache) {
      if (!key.startsWith('native/')) continue;
      updateInfoCache.set(key, {
        ...cacheEntry,
        sources: cacheEntry.sources.map(clearNativeGithubProviderCooldown),
      });
    }
    set((state) => ({
      snapshots: Object.fromEntries(Object.entries(state.snapshots).map(([key, snapshot]) => {
        if (!key.startsWith('native/')) return [key, snapshot];
        return [key, {
          ...snapshot,
          skills: snapshot.skills.map((skill) => ({
            ...skill,
            updateEvidence: skill.updateEvidence
              ? clearNativeGithubProviderCooldown(skill.updateEvidence)
              : skill.updateEvidence,
          })),
          updateCheck: snapshot.updateCheck
            ? {
                ...snapshot.updateCheck,
                sources: snapshot.updateCheck.sources.map(clearNativeGithubProviderCooldown),
              }
            : snapshot.updateCheck,
        }];
      })),
    }));
  },
  refreshContext: async (context, options = {}) => {
    const origin = options.origin ?? 'passive';
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
      let committed = false;
      set((state) => {
        if (state.snapshots[key]?.requestId !== requestId) return {};
        committed = true;
        const nextSnapshot = {
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
        };
        return {
          snapshots: {
            ...state.snapshots,
            [key]: nextSnapshot,
          },
        };
      });
      if (!committed) return;
      if (origin === 'selfMutation') {
        set((state) => {
          const eligible = eligibleSkills({ ...current, skills });
          const existingSession = state.updateCheckSessions[key];
          const session = existingSession ?? emptyUpdateCheckSession();
          const mutatedNames = new Set(options.mutatedSkillNames);
          const observedSkills = { ...session.observedSkills };
          for (const skillName of mutatedNames) delete observedSkills[skillName];
          for (const skill of eligible) {
            if (mutatedNames.has(skill.name)) {
              observedSkills[skill.name] = skillCheckFingerprint(context, skill);
            }
          }
          const ownsEligibleResult = eligible.some((skill) => mutatedNames.has(skill.name));
          return {
            updateCheckSessions: {
              ...state.updateCheckSessions,
              [key]: {
                ...session,
                initialAttempted: session.initialAttempted || ownsEligibleResult,
                observedSkills,
              },
            },
          };
        });
        await get().reconcileAutomaticChecks(context);
      } else {
        await get().reconcileAutomaticChecks(context);
      }
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

  refreshWorkspace: async (context, options = { origin: 'initial' }) => {
    if (context.scope.scope === 'global') {
      await get().refreshContext(context, options);
      return;
    }
    await Promise.all([
      get().refreshContext(
        globalContext(context.environment),
        options.origin === 'selfMutation' ? { origin: 'passive' } : options,
      ),
      get().refreshContext(context, options),
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

  syncSkills: async (context, options = { origin: 'passive' }) => {
    set({ isSyncing: true });
    try {
      await get().refreshWorkspace(context, options);
    } finally {
      set({ isSyncing: false });
    }
  },

  syncUpdates: async (context) => {
    await get().activateAutomaticChecks(context);
  },

  activateAutomaticChecks: async (context) => {
    const key = contextKey(context);
    set((state) => ({
      updateCheckSessions: {
        ...state.updateCheckSessions,
        [key]: {
          ...(state.updateCheckSessions[key] ?? emptyUpdateCheckSession()),
          active: true,
        },
      },
    }));
    await get().reconcileAutomaticChecks(context);
  },

  reconcileAutomaticChecks: async (context) => {
    const key = contextKey(context);
    let selection: UpdateCheckSelection | null = null;
    set((state) => {
      const session = state.updateCheckSessions[key] ?? emptyUpdateCheckSession();
      const snapshot = state.snapshots[key];
      if (
        !session.active
        || session.automaticPending
        || session.forcePending
        || !snapshot
        || snapshot.loading
        || snapshot.error
      ) {
        return {};
      }
      const eligible = eligibleSkills(snapshot);
      if (eligible.length === 0) {
        if (session.initialAttempted && Object.keys(session.observedSkills).length > 0) {
          return {
            updateCheckSessions: {
              ...state.updateCheckSessions,
              [key]: { ...session, observedSkills: {} },
            },
          };
        }
        return {};
      }
      const nextObserved = Object.fromEntries(
        eligible.map((skill) => [
          skill.name,
          skillCheckFingerprint(context, skill),
        ]),
      );
      if (!session.initialAttempted) {
        selection = { kind: 'all' };
      } else {
        const changed = eligible.filter((skill) => (
          state.updateCheckSessions[key]?.observedSkills[skill.name]
            !== nextObserved[skill.name]
        ));
        const observedChanged = Object.keys(session.observedSkills).length
          !== Object.keys(nextObserved).length
          || Object.entries(nextObserved).some(([name, fingerprint]) => (
            session.observedSkills[name] !== fingerprint
          ));
        if (changed.length === 0) {
          if (!observedChanged) return {};
          return {
            updateCheckSessions: {
              ...state.updateCheckSessions,
              [key]: { ...session, observedSkills: nextObserved },
            },
          };
        }
        selection = {
          kind: 'skills',
          skills: changed.map((skill) => ({ context, skillName: skill.name })),
        };
      }
      const nextSessions = {
        ...state.updateCheckSessions,
        [key]: {
          ...session,
          initialAttempted: true,
          observedSkills: nextObserved,
          automaticPending: true,
        },
      };
      const scopes = pendingScopes(nextSessions);
      return {
        updateCheckSessions: nextSessions,
        checkingUpdateScopes: scopes.checking,
        automaticUpdateScopes: scopes.automatic,
        forceUpdateScopes: scopes.force,
      };
    });
    const admittedSelection = selection as UpdateCheckSelection | null;
    if (!admittedSelection) return;
    const ticket = beginUpdateCheckRequest(key, 0);
    try {
      let result: UpdateCheckResult;
      try {
        result = {
          ok: true,
          response: await checkUpdates({ context, mode: 'automatic', selection: admittedSelection }),
        };
      } catch (error) {
        toast.error(t('skills.checkUpdatesError', {
          error: error instanceof Error ? error.message : String(error),
        }));
        result = { ok: false };
      }
      if (!result.ok || !isAdmittedUpdateCheckRequest(ticket)) return;
      const previous = updateInfoCache.get(key);
      const retainedResults = admittedSelection.kind === 'skills'
        ? (previous?.results ?? []).filter((item) => (
            !admittedSelection.skills.some((selected) => selected.skillName === item.name)
          ))
        : [];
      const results = admittedSelection.kind === 'skills'
        ? [
            ...retainedResults,
            ...preserveLastConfirmedUpdates(previous?.results ?? [], result.response.skills),
          ]
        : preserveLastConfirmedUpdates(previous?.results ?? [], result.response.skills);
      const sources = admittedSelection.kind === 'skills'
        ? mergeSourceUpdateInfo(previous?.sources ?? [], result.response.sources, retainedResults)
        : result.response.sources;
      const completeness: 'partial' | 'complete' = admittedSelection.kind === 'skills'
        ? 'partial'
        : 'complete';
      const cacheEntry = {
        results,
        sources,
        checkedAt: Date.now(),
        completeness,
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
                preserveUnmatched: completeness === 'partial',
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
      set((state) => {
        const session = state.updateCheckSessions[key];
        if (!session) return {};
        const nextSessions = {
          ...state.updateCheckSessions,
          [key]: { ...session, automaticPending: false },
        };
        const scopes = pendingScopes(nextSessions);
        return {
          updateCheckSessions: nextSessions,
          checkingUpdateScopes: scopes.checking,
          automaticUpdateScopes: scopes.automatic,
          forceUpdateScopes: scopes.force,
        };
      });
      finishUpdateCheckRequest(ticket);
      void get().reconcileAutomaticChecks(context);
    }
  },

  forceCheckUpdates: async (context, selection) => {
    const cacheKey = contextKey(context);
    const admitted = (() => {
      let result = false;
      set((state) => {
        const session = state.updateCheckSessions[cacheKey] ?? emptyUpdateCheckSession();
        if (session.forcePending) return {};
        const nextSessions = {
          ...state.updateCheckSessions,
          [cacheKey]: { ...session, forcePending: true },
        };
        const scopes = pendingScopes(nextSessions);
        result = true;
        return {
          updateCheckSessions: nextSessions,
          checkingUpdateScopes: scopes.checking,
          automaticUpdateScopes: scopes.automatic,
          forceUpdateScopes: scopes.force,
        };
      });
      return result;
    })();
    if (!admitted) return null;
    const ticket = beginUpdateCheckRequest(cacheKey, 1);

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
      const retainedResults = selection.kind === 'skills'
        ? (previous?.results ?? []).filter((item) => (
            !selection.skills.some((selected) => selected.skillName === item.name)
          ))
        : [];
      const results = selection.kind === 'skills'
        ? [
            ...retainedResults,
            ...updates,
          ]
        : updates;
      const sources = selection.kind === 'skills'
        ? mergeSourceUpdateInfo(previous?.sources ?? [], response.sources, retainedResults)
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
      set((state) => {
        const session = state.updateCheckSessions[cacheKey];
        if (!session) return {};
        const nextSessions = {
          ...state.updateCheckSessions,
          [cacheKey]: { ...session, forcePending: false },
        };
        const scopes = pendingScopes(nextSessions);
        return {
          updateCheckSessions: nextSessions,
          checkingUpdateScopes: scopes.checking,
          automaticUpdateScopes: scopes.automatic,
          forceUpdateScopes: scopes.force,
        };
      });
      finishUpdateCheckRequest(ticket);
      void get().reconcileAutomaticChecks(context);
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
        .filter((skill) => successfulSkillNames.has(skill.name)
          && (skill.updateReason === 'missingRemoteHash' || skill.updateReason === 'missing-remote-hash'))
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
    await get().refreshContext(context, {
      origin: 'selfMutation',
      mutatedSkillNames: Array.from(successfulSkillNames),
    });
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
