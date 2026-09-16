import { create } from 'zustand';
import { toast } from 'sonner';
import { sortSkills, mergeUpdateInfo, type SkillListItem, type UpdateCheckDisplaySnapshot, t } from './skills-utils';
import { listSkills, checkUpdates } from '@/hooks/useTauriApi';
import { toAppError } from '@/utils/to-app-error';
import { updateCheckFailureKey } from '@/lib/skill-status-presentation';
import { contextKey, globalContext } from '@/lib/context';
import type {
  AppError, SkillLocationRef, ResolvedAgent, SourceUpdateCheckInfo,
  UpdateCheckSelection, UpdateCheckOutcome, UpdateResponse,
  LibraryApplicationSummary,
  ScopePathBase,
  SkillReadStatus,
} from '@/bindings';

export type RefreshOrigin = 'initial' | 'passive' | 'selfMutation';
export type RefreshOptions =
  | { origin?: Exclude<RefreshOrigin, 'selfMutation'>; mutatedSkillNames?: never }
  | { origin: 'selfMutation'; mutatedSkillNames: string[]; invalidateUpdates?: boolean };

export interface UpdateCheckSession {
  active: boolean;
  observedSkills: Record<string, string>;
}

export interface ContextSkillSnapshot {
  pathBase?: ScopePathBase | null;
  skills: SkillListItem[];
  updateCheck?: UpdateCheckDisplaySnapshot;
  agents: ResolvedAgent[];
  libraryApplication?: LibraryApplicationSummary;
  pathExists: boolean;
  readStatus?: SkillReadStatus | null;
  loading: boolean;
  error: AppError | null;
  requestId: number;
}

type UpdateCheckIntent = UpdateCheckSelection | { kind: 'all' };
interface SkillsDataState {
  snapshots: Record<string, ContextSkillSnapshot>;
  updateCheckSessions: Record<string, UpdateCheckSession>;
  isSyncing: boolean;
  automaticUpdateScopes: Set<string>;
  forceUpdateScopes: Set<string>;
  refreshContext: (context: SkillLocationRef, options?: RefreshOptions) => Promise<void>;
  refreshWorkspace: (context: SkillLocationRef, options?: RefreshOptions) => Promise<void>;
  invalidateContexts: (contexts: SkillLocationRef[]) => void;
  invalidateAgentProjections: () => void;
  syncSkills: (context: SkillLocationRef, options?: RefreshOptions) => Promise<void>;
  activateAutomaticChecks: (context: SkillLocationRef) => Promise<void>;
  reconcileAutomaticChecks: (context: SkillLocationRef) => Promise<void>;
  forceCheckUpdates: (context: SkillLocationRef, selection: UpdateCheckIntent) => Promise<UpdateCheckOutcome | null>;
  applyUpdateResult: (context: SkillLocationRef, response: UpdateResponse) => Promise<void>;
  clearNativeGithubProviderCooldown: () => void;
}

const emptySession = (): UpdateCheckSession => ({
  active: false, observedSkills: {},
});
const emptySnapshot = (): ContextSkillSnapshot => ({
  skills: [], agents: [], pathExists: true, loading: false, error: null, requestId: 0,
  libraryApplication: { orderedLibraries: [], selectedAgentIds: [], pending: false, syncState: 'synced' },
});
const eligible = (snapshot: ContextSkillSnapshot) => snapshot.skills.filter((skill) => skill.canCheckForUpdates === true);
const fingerprint = (skill: SkillListItem) => skill.comparisonFingerprint ?? '';
const sourceKey = (source: SourceUpdateCheckInfo) => source.sourceKey ?? source.source;

export const useSkillsDataStore = create<SkillsDataState>()((set, get) => {
  let sequence = 0;
  const epochs = new Map<string, number>();
  const pending = new Map<string, { key: string; mode: 'force' | 'automatic'; promise: Promise<UpdateCheckOutcome | null> }>();

  function publishPending() {
    set(() => {
      const automatic = new Set<string>();
      const force = new Set<string>();
      for (const item of pending.values()) {
        (item.mode === 'force' ? force : automatic).add(item.key);
      }
      return { automaticUpdateScopes: automatic, forceUpdateScopes: force };
    });
  }

  function runCheck(context: SkillLocationRef, names: string[], mode: 'force' | 'automatic'): Promise<UpdateCheckOutcome | null> {
    const key = contextKey(context);
    const selected = [...new Set(names)].sort();
    if (!selected.length) return Promise.resolve(null);
    const initial = get().snapshots[key];
    const baselines = new Map(initial?.skills.map((skill) => [skill.name, fingerprint(skill)]));
    const epoch = epochs.get(key) ?? 0;
    const requestKey = JSON.stringify([key, epoch, mode, selected.map((name) => [name, baselines.get(name)])]);
    const existing = pending.get(requestKey);
    if (existing) return existing.promise;
    const requestId = ++sequence;
    set((state) => {
      const snapshot = state.snapshots[key];
      if (!snapshot) return {};
      const check = snapshot.updateCheck;
      const memberRequests = { ...check?.memberRequests };
      for (const name of selected) memberRequests[name] = requestId;
      return { snapshots: { ...state.snapshots, [key]: { ...snapshot, updateCheck: {
        sources: check?.sources ?? [],
        checkedAt: check?.checkedAt ?? 0,
        results: check?.results ?? [], memberRequests,
        invalidatedAt: check?.invalidatedAt,
        comparisonObservedAt: check?.comparisonObservedAt,
      } } } };
    });

    // Defer invocation until the pending ticket is visible, including synchronous mock failures.
    const promise = Promise.resolve().then(async () => {
      try {
        const response = await checkUpdates({ context, mode, selection: { kind: 'skills', skills: selected.map((skillName) => ({ context, skillName })) } });
        let needsRefresh = false;
        let acceptedResponse = false;
        set((state) => {
          const current = state.snapshots[key];
          if (!current || (epochs.get(key) ?? 0) !== epoch) return {};
          const check = current.updateCheck;
          const results = new Map(check?.results?.map((result) => [result.name, result]));
          const sources = new Map(check?.sources.map((source) => [sourceKey(source), source]));
          const incomingSources = new Map(response.sources.map((source) => [sourceKey(source), source]));
          const comparisonObservedAt = { ...check?.comparisonObservedAt };
          const accepted = [];
          for (const name of selected) {
            if (check?.memberRequests?.[name] !== requestId || (check.invalidatedAt?.[name] ?? 0) > requestId) continue;
            const skill = current.skills.find((skill) => skill.name === name);
            const result = response.skills.find((result) => result.name === name);
            if (skill && (!result || (result.comparisonFingerprint ?? '') !== fingerprint(skill))) needsRefresh = true;
          }
          for (const result of response.skills) {
            const skill = current.skills.find((skill) => skill.name === result.name);
            if (!skill || !selected.includes(result.name) || fingerprint(skill) !== baselines.get(result.name)
              || (result.comparisonFingerprint ?? '') !== fingerprint(skill)
              || (check?.invalidatedAt?.[result.name] ?? 0) > requestId) continue;
            const latestRequest = check?.memberRequests?.[result.name] ?? requestId;
            const previous = results.get(result.name);
            const incoming = result.sourceKey ? incomingSources.get(result.sourceKey) : undefined;
            const observedAt = incoming?.checkedAtEpochMs ?? 0;
            const previousObservedAt = comparisonObservedAt[result.name] ?? 0;
            const newerEvidence = observedAt > previousObservedAt;
            if (latestRequest > requestId && !newerEvidence) continue;
            let merged = latestRequest > requestId && previous
              ? { ...result, error: previous.error, freshness: previous.freshness }
              : result;
            if (previous && previous.status !== 'cannotCheck' && (observedAt < previousObservedAt || result.status === 'cannotCheck')) {
              merged = { ...merged, hasUpdate: previous.hasUpdate, status: previous.status, reason: previous.reason };
            }
            if (result.status !== 'cannotCheck') comparisonObservedAt[result.name] = Math.max(previousObservedAt, observedAt);
            results.set(result.name, merged);
            accepted.push(merged);
            if (incoming) {
              const previousSource = sources.get(sourceKey(incoming));
              if (!previousSource || (incoming.checkedAtEpochMs ?? 0) >= (previousSource.checkedAtEpochMs ?? 0)) sources.set(sourceKey(incoming), incoming);
            }
          }
          const allResults = [...results.values()];
          if (accepted.length === 0) return {};
          acceptedResponse = true;
          const allSources = [...sources.values()];
          const nextSkills = sortSkills(mergeUpdateInfo(current.skills, accepted, { preserveUnmatched: true, sources: allSources }));
          const currentNames = new Set(current.skills.map((skill) => skill.name));
          return { snapshots: { ...state.snapshots, [key]: { ...current, skills: nextSkills, updateCheck: {
            sources: allSources, results: allResults.filter((result) => currentNames.has(result.name)),
            checkedAt: Date.now(), memberRequests: check?.memberRequests,
            invalidatedAt: check?.invalidatedAt,
            comparisonObservedAt,
          } } } };
        });
        if (needsRefresh) await get().refreshContext(context);
        if (mode === 'force' && acceptedResponse && response.outcome !== 'completed') {
          toast.error(t(updateCheckFailureKey(response)));
        }
        return response.outcome;
      } catch (error) {
        let currentFailure = false;
        set((state) => {
          const snapshot = state.snapshots[key];
          if (!snapshot?.updateCheck || (epochs.get(key) ?? 0) !== epoch) return {};
          if (!selected.some((name) => snapshot.updateCheck?.memberRequests?.[name] === requestId && (snapshot.updateCheck.invalidatedAt?.[name] ?? 0) <= requestId)) return {};
          currentFailure = true;
          return { snapshots: { ...state.snapshots, [key]: { ...snapshot, updateCheck: { ...snapshot.updateCheck, error: toAppError(error), checkedAt: Date.now() } } } };
        });
        if (mode === 'force' && currentFailure) toast.error(t(updateCheckFailureKey(toAppError(error))));
        return null;
      } finally {
        pending.delete(requestKey);
        publishPending();
      }
    });
    pending.set(requestKey, { key, mode, promise });
    publishPending();
    return promise;
  }

  return {
    snapshots: {}, updateCheckSessions: {}, isSyncing: false,
    automaticUpdateScopes: new Set(), forceUpdateScopes: new Set(),
    clearNativeGithubProviderCooldown: () => {
      const clear = (source: SourceUpdateCheckInfo): SourceUpdateCheckInfo => {
        const failure = source.lastAttempt?.failure;
        if (!failure?.providerCooldown || source.provider !== 'github') return source;
        return { ...source, lastAttempt: { ...source.lastAttempt!, failure: { ...failure, retryAtEpochMs: null, providerCooldown: false } } };
      };
      set((state) => ({ snapshots: Object.fromEntries(Object.entries(state.snapshots).map(([key, snapshot]) => !key.startsWith('native/') ? [key, snapshot] : [key, {
        ...snapshot, skills: snapshot.skills.map((skill) => ({ ...skill, updateEvidence: skill.updateEvidence ? clear(skill.updateEvidence) : skill.updateEvidence })),
        updateCheck: snapshot.updateCheck ? { ...snapshot.updateCheck, sources: snapshot.updateCheck.sources.map(clear) } : undefined,
      }])) }));
    },
    refreshContext: async (context, options = {}) => {
      const key = contextKey(context);
      const requestId = ++sequence;
      if (options.origin === 'selfMutation' && options.invalidateUpdates) {
        const names = new Set(options.mutatedSkillNames);
        set((state) => {
          const snapshot = state.snapshots[key];
          if (!snapshot) return {};
          const check = snapshot.updateCheck;
          const memberRequests = { ...check?.memberRequests };
          for (const name of names) memberRequests[name] = requestId;
          return { snapshots: { ...state.snapshots, [key]: {
            ...snapshot, skills: snapshot.skills.map((skill) => names.has(skill.name) ? {
              ...skill, hasUpdate: false, updateStatus: null, updateReason: null, updateFreshness: null, updateEvidence: null, updateAttempt: null, updateError: null,
            } : skill),
            updateCheck: check ? { ...check, results: check.results?.filter((result) => !names.has(result.name)), memberRequests,
              invalidatedAt: { ...check.invalidatedAt, ...Object.fromEntries([...names].map((name) => [name, requestId])) },
            } : undefined,
          } } };
        });
      }
      set((state) => ({ snapshots: { ...state.snapshots, [key]: { ...(state.snapshots[key] ?? emptySnapshot()), loading: true, error: null, requestId } } }));
      try {
        const result = await listSkills(context);
        set((state) => {
          const previous = state.snapshots[key];
          if (previous?.requestId !== requestId) return {};
          const checks = previous.updateCheck;
          const results = (checks?.results ?? []).filter((update) => result.skills.some((skill) => skill.name === update.name && fingerprint(skill) === (update.comparisonFingerprint ?? '')));
          const skills = sortSkills(mergeUpdateInfo(result.skills, results, { preserveUnmatched: true, previousSkills: previous.skills, sources: checks?.sources }));
          return { snapshots: { ...state.snapshots, [key]: {
            ...previous, ...result, skills, loading: false, error: null,
            updateCheck: checks ? { ...checks, results } : undefined,
          } } };
        });
        if (options.origin === 'selfMutation') {
          const { useSkillDetailStore } = await import('./skill-detail');
          const detail = useSkillDetailStore.getState();
          if (get().snapshots[key]?.requestId === requestId
            && detail.selectedContext && contextKey(detail.selectedContext) === key
            && detail.selectedSkillRef && options.mutatedSkillNames.includes(detail.selectedSkillRef.name)) {
            await detail.reloadContent();
          }
        }
        await get().reconcileAutomaticChecks(context);
      } catch (error) {
        set((state) => state.snapshots[key]?.requestId !== requestId ? {} : {
          snapshots: { ...state.snapshots, [key]: { ...state.snapshots[key]!, loading: false, error: toAppError(error) } },
        });
      }
    },
    refreshWorkspace: async (context, options = { origin: 'initial' }) => {
      if (context.scope.scope === 'global') return get().refreshContext(context, options);
      await Promise.all([
        get().refreshContext(globalContext(context.environment), options.origin === 'selfMutation' ? { origin: 'passive' } : options),
        get().refreshContext(context, options),
      ]);
    },
    invalidateContexts: (contexts) => {
      const keys = new Set(contexts.map(contextKey));
      for (const key of keys) epochs.set(key, (epochs.get(key) ?? 0) + 1);
      set((state) => ({ snapshots: Object.fromEntries(Object.entries(state.snapshots).filter(([key]) => !keys.has(key))),
        updateCheckSessions: Object.fromEntries(Object.entries(state.updateCheckSessions).map(([key, session]) => [key, keys.has(key) ? { ...emptySession(), active: session.active } : session])),
      }));
    },
    invalidateAgentProjections: () => {
      for (const key of Object.keys(get().snapshots)) epochs.set(key, (epochs.get(key) ?? 0) + 1);
      set((state) => ({ snapshots: {}, updateCheckSessions: Object.fromEntries(Object.entries(state.updateCheckSessions).map(([key, session]) => [key, { ...emptySession(), active: session.active }])) }));
    },
    syncSkills: async (context, options = { origin: 'passive' }) => {
      set({ isSyncing: true });
      try { await get().refreshWorkspace(context, options); } finally { set({ isSyncing: false }); }
    },
    activateAutomaticChecks: async (context) => {
      const key = contextKey(context);
      set((state) => ({ updateCheckSessions: Object.fromEntries([
        ...Object.entries(state.updateCheckSessions).map(([other, session]) => [other, { ...session, active: false }]),
        [key, { ...(state.updateCheckSessions[key] ?? emptySession()), active: true }],
      ]) }));
      await get().reconcileAutomaticChecks(context);
    },
    reconcileAutomaticChecks: async (context) => {
      const key = contextKey(context);
      const state = get();
      const session = state.updateCheckSessions[key];
      const snapshot = state.snapshots[key];
      if (!session?.active || !snapshot || snapshot.loading || snapshot.error) return;
      const now = Date.now();
      const names = eligible(snapshot).filter((skill) => {
        const changed = session.observedSkills[skill.name] !== fingerprint(skill);
        if (!changed && skill.updateError?.kind === 'capabilityUnavailable'
          && ['sourceDirectoryLinks', 'sourceSessionCapacity'].includes(skill.updateError.data.capability)) return false;
        const evidence = skill.updateEvidence;
        const failure = evidence?.lastAttempt?.failure;
        if (!changed && failure && ['authenticationRequired', 'refNotFound', 'repositoryNotFound', 'notFoundOrUnauthorized'].includes(failure.reason)) return false;
        const deadline = failure?.retryAtEpochMs ?? evidence?.expiresAtEpochMs;
        if (!changed && deadline != null) return deadline <= now;
        return changed
          || (skill.updateAttempt?.outcome === 'notCompleted' && now - (skill.updateAttempt.attemptedAt ?? now) >= 30_000)
          || (snapshot.updateCheck?.error != null && now - snapshot.updateCheck.checkedAt >= 30_000);
      }).map((skill) => skill.name);
      if (!names.length) return;
      set((state) => ({ updateCheckSessions: { ...state.updateCheckSessions, [key]: {
        ...session, observedSkills: Object.fromEntries(eligible(snapshot).map((skill) => [skill.name, fingerprint(skill)])),
      } } }));
      await runCheck(context, names, 'automatic');
    },
    forceCheckUpdates: (context, selection) => {
      const snapshot = get().snapshots[contextKey(context)] ?? emptySnapshot();
      const names = eligible(snapshot).filter((skill) => selection.kind === 'all' || selection.skills.some((item) => item.skillName === skill.name && contextKey(item.context) === contextKey(context))).map((skill) => skill.name);
      return runCheck(context, names, 'force');
    },
    applyUpdateResult: async (context, response) => {
      const names = response.skills.filter((item) => item.mutation?.status === 'succeeded').map((item) => item.skillIdentity.skillName);
      await get().refreshContext(context, { origin: 'selfMutation', mutatedSkillNames: names, invalidateUpdates: true });
    },
  };
});
