import { beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  SkillLocationRef,
  MutationUnitResult,
  SkillUpdateInfo,
  SourceUpdateCheckInfo,
  UpdateCheckResponse,
  UpdateCheckSelection,
  UpdateResponse,
} from '@/bindings';
import { contextKey } from '@/lib/context';
import { useSkillsDataStore } from '../skills-data';
import { mergeUpdateInfo, type SkillListItem } from '../skills-utils';

const mocks = vi.hoisted(() => ({
  listSkills: vi.fn(),
  listAgents: vi.fn(),
  checkUpdates: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  listSkills: (...args: unknown[]) => mocks.listSkills(...args),
  listAgents: (...args: unknown[]) => mocks.listAgents(...args),
  checkUpdates: (...args: unknown[]) => mocks.checkUpdates(...args),
}));

vi.mock('sonner', () => ({ toast: { error: mocks.toastError } }));

const context: SkillLocationRef = {
  environment: { kind: 'native' },
  scope: { scope: 'global' },
};

function selected(
  target: SkillLocationRef,
  names: string[] = ['toolkit'],
): UpdateCheckSelection {
  return {
    kind: 'skills',
    skills: names.map((skillName) => ({ context: target, skillName })),
  };
}

function skill(overrides: Partial<SkillListItem> = {}): SkillListItem {
  return {
    name: 'toolkit', description: '', path: '/skills/toolkit', canonicalPath: '/canonical/toolkit',
    scope: 'global', agents: ['codex'], associatedAgents: ['codex'], source: 'owner/repo', hasUpdate: true,
    canRunUpdate: true, canCheckForUpdates: true, updateStatus: 'updateAvailable',
    comparisonFingerprint: 'baseline-1', updateReason: null, ...overrides,
  };
}

function updateResponse(status: MutationUnitResult['status']): UpdateResponse {
  return {
    sources: [],
    skills: [{
      skillIdentity: { context, skillName: 'toolkit' }, sourceResultId: '',
      mutation: {
        unitId: 'toolkit', skillName: 'toolkit', source: null, target: context, status,
        retryable: status !== 'succeeded', lockCommitted: status === 'succeeded',
        actualMode: null, fallbackReason: null, agentTargets: [], warnings: [], error: null, recovery: null,
      },
      coverage: { kind: 'updated' }, warnings: [], retryable: status !== 'succeeded',
    }],
    outcome: status === 'succeeded' ? 'succeeded' : 'failed',
  };
}

function setSkills(skills: SkillListItem[]) {
  useSkillsDataStore.setState({
    snapshots: {
      [contextKey(context)]: { skills, agents: [], pathExists: true, loading: false, error: null, requestId: 1 },
    },
  });
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((next, fail) => { resolve = next; reject = fail; });
  return { promise, resolve, reject };
}

function updateInfo(
  name: string,
  overrides: Partial<SkillUpdateInfo> = {},
): SkillUpdateInfo {
  return {
    name,
    source: `${name}/repo`,
    hasUpdate: false,
    status: 'upToDate',
    capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
    reason: null,
    gitRef: null,
    sourceUrl: null,
    skillPath: `skills/${name}`,
    freshness: 'fresh',
    comparisonFingerprint: 'baseline-1', sourceKey: 'source-1',
    ...overrides,
  };
}

function sourceInfo(
  source: string,
  overrides: Partial<SourceUpdateCheckInfo> = {},
): SourceUpdateCheckInfo {
  return {
    source,
    requestedRef: 'HEAD',
    resolvedRef: 'main',
    refRevision: 'revision-1',
    sourceKey: 'source-1', checkedAtEpochMs: Date.now(),
    expiresAtEpochMs: Date.now() + 3_600_000,
    freshness: 'fresh',
    lastAttempt: null,
    ...overrides,
  };
}

describe('skills data store', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useSkillsDataStore.setState({
      snapshots: {}, updateCheckSessions: {}, isSyncing: false,
      automaticUpdateScopes: new Set(), forceUpdateScopes: new Set(),
    });
    mocks.listSkills.mockResolvedValue({ skills: [], agents: [], pathExists: true });
    mocks.checkUpdates.mockResolvedValue({ outcome: 'completed', sources: [sourceInfo('source')], skills: [updateInfo('toolkit')] });
  });

  const force = (names = ['toolkit']) => useSkillsDataStore.getState().forceCheckUpdates(context, selected(context, names));

  it('reports an unsuccessful manual check without changing the known update', async () => {
    setSkills([skill()]);
    mocks.checkUpdates.mockResolvedValue({ ...response([updateInfo('toolkit', {
      status: 'cannotCheck', reason: 'upstreamUnavailable', freshness: 'unavailable',
      error: { kind: 'gitNetworkError', data: { message: 'offline' } },
    })]), outcome: 'notCompleted' });
    await force();
    expect(mocks.toastError).toHaveBeenCalledOnce();
    expect(snapshot().skills[0].hasUpdate).toBe(true);
  });
  const snapshot = () => useSkillsDataStore.getState().snapshots[contextKey(context)]!;
  const response = (skills: SkillUpdateInfo[], sourceOverrides: Partial<SourceUpdateCheckInfo> = {}): UpdateCheckResponse => ({
    outcome: 'completed', skills, sources: [sourceInfo('source', sourceOverrides)],
  });

  it('matches backend fingerprints and source keys without interpreting URLs', () => {
    const info = updateInfo('toolkit', { source: 'same display', sourceKey: 'query-a', hasUpdate: true, status: 'updateAvailable' });
    const merged = mergeUpdateInfo([skill({ hasUpdate: false })], [info], { sources: [
      sourceInfo('same display', { sourceKey: 'query-b', refRevision: 'wrong' }),
      sourceInfo('different display', { sourceKey: 'query-a', refRevision: 'correct' }),
    ] });
    expect(merged[0]?.updateEvidence?.refRevision).toBe('correct');
    expect(mergeUpdateInfo([skill({ comparisonFingerprint: 'new-baseline', hasUpdate: false, updateStatus: null })], [info])[0]?.hasUpdate).toBe(false);
  });

  it('waits for eligible canonical installations before making an automatic request', async () => {
    await useSkillsDataStore.getState().activateAutomaticChecks(context);
    expect(mocks.checkUpdates).not.toHaveBeenCalled();
    setSkills([skill({ canCheckForUpdates: false })]);
    await useSkillsDataStore.getState().reconcileAutomaticChecks(context);
    expect(mocks.checkUpdates).not.toHaveBeenCalled();
    setSkills([skill()]);
    await useSkillsDataStore.getState().reconcileAutomaticChecks(context);
    expect(mocks.checkUpdates).toHaveBeenCalledOnce();
  });

  it('reuses fresh comparisons across activation and passive refresh', async () => {
    setSkills([skill()]);
    mocks.listSkills.mockResolvedValue({ skills: [skill({ hasUpdate: false, updateStatus: null })], agents: [], pathExists: true });
    await useSkillsDataStore.getState().activateAutomaticChecks(context);
    await useSkillsDataStore.getState().activateAutomaticChecks(context);
    await useSkillsDataStore.getState().refreshContext(context);
    expect(mocks.checkUpdates).toHaveBeenCalledOnce();
  });

  it('checks an expired visible location again', async () => {
    setSkills([skill()]);
    mocks.checkUpdates.mockResolvedValue(response([updateInfo('toolkit')], { expiresAtEpochMs: Date.now() - 1 }));
    await useSkillsDataStore.getState().activateAutomaticChecks(context);
    await useSkillsDataStore.getState().reconcileAutomaticChecks(context);
    expect(mocks.checkUpdates).toHaveBeenCalledTimes(2);
  });

  it('starts a new request when an installation baseline changes during a pending check', async () => {
    setSkills([skill()]);
    const old = deferred<UpdateCheckResponse>();
    mocks.checkUpdates.mockReturnValueOnce(old.promise).mockResolvedValueOnce(response([updateInfo('toolkit', { comparisonFingerprint: 'baseline-2' })]));
    const first = useSkillsDataStore.getState().activateAutomaticChecks(context);
    await Promise.resolve();
    mocks.listSkills.mockResolvedValue({ skills: [skill({ comparisonFingerprint: 'baseline-2', hasUpdate: false, updateStatus: null })], agents: [], pathExists: true });
    await useSkillsDataStore.getState().refreshContext(context);
    expect(mocks.checkUpdates).toHaveBeenCalledTimes(2);
    old.resolve(response([updateInfo('toolkit', { hasUpdate: true, status: 'updateAvailable' })]));
    await first;
    expect(snapshot().skills[0]?.hasUpdate).toBe(false);
    expect(snapshot().skills[0]?.comparisonFingerprint).toBe('baseline-2');
  });

  it('only automatically checks the active location', async () => {
    const project: SkillLocationRef = { ...context, scope: { scope: 'project', project_id: 'p' } };
    setSkills([skill()]);
    useSkillsDataStore.setState((state) => ({ snapshots: { ...state.snapshots, [contextKey(project)]: { ...snapshot(), skills: [skill({ scope: 'project' })] } } }));
    await useSkillsDataStore.getState().activateAutomaticChecks(project);
    await useSkillsDataStore.getState().reconcileAutomaticChecks(context);
    expect(mocks.checkUpdates).toHaveBeenCalledOnce();
    expect(mocks.checkUpdates).toHaveBeenCalledWith(expect.objectContaining({ context: project }));
  });

  it('deduplicates only an identical pending request', async () => {
    setSkills([skill()]);
    const remote = deferred<UpdateCheckResponse>();
    mocks.checkUpdates.mockReturnValue(remote.promise);
    const first = force();
    const second = force();
    await Promise.resolve();
    expect(mocks.checkUpdates).toHaveBeenCalledOnce();
    remote.resolve(response([updateInfo('toolkit')]));
    await Promise.all([first, second]);
  });

  it('submits a manual request while automatic checking is pending', async () => {
    setSkills([skill()]);
    const automatic = deferred<UpdateCheckResponse>();
    const manual = deferred<UpdateCheckResponse>();
    mocks.checkUpdates.mockReturnValueOnce(automatic.promise).mockReturnValueOnce(manual.promise);
    const first = useSkillsDataStore.getState().activateAutomaticChecks(context);
    await Promise.resolve();
    const second = force();
    await Promise.resolve();
    expect(mocks.checkUpdates.mock.calls.map(([request]) => request.mode)).toEqual(['automatic', 'force']);
    manual.resolve(response([updateInfo('toolkit', { hasUpdate: true, status: 'updateAvailable' })], { checkedAtEpochMs: 200 }));
    await second;
    expect(useSkillsDataStore.getState().automaticUpdateScopes.has(contextKey(context))).toBe(true);
    expect(useSkillsDataStore.getState().forceUpdateScopes.has(contextKey(context))).toBe(false);
    automatic.resolve(response([updateInfo('toolkit')], { checkedAtEpochMs: 100 }));
    await first;
    expect(snapshot().skills[0]?.hasUpdate).toBe(true);
    expect(useSkillsDataStore.getState().automaticUpdateScopes.size).toBe(0);
    expect(useSkillsDataStore.getState().forceUpdateScopes.size).toBe(0);
  });

  it('lets independent manual selections complete without waiting for the slow member', async () => {
    setSkills([skill({ name: 'alpha' }), skill({ name: 'beta' })]);
    const slow = deferred<UpdateCheckResponse>();
    mocks.checkUpdates.mockReturnValueOnce(slow.promise).mockResolvedValueOnce(response([updateInfo('beta', { hasUpdate: true, status: 'updateAvailable', sourceKey: 'beta-source' })], { sourceKey: 'beta-source' }));
    const first = force(['alpha']);
    await Promise.resolve();
    await force(['beta']);
    expect(mocks.checkUpdates).toHaveBeenCalledTimes(2);
    expect(snapshot().skills.find((skill) => skill.name === 'beta')?.hasUpdate).toBe(true);
    expect(useSkillsDataStore.getState().forceUpdateScopes.has(contextKey(context))).toBe(true);
    slow.resolve(response([updateInfo('alpha')]));
    await first;
    expect(snapshot().skills.find((skill) => skill.name === 'alpha')?.hasUpdate).toBe(false);
    expect(snapshot().skills.find((skill) => skill.name === 'beta')?.hasUpdate).toBe(true);
  });

  it('retains unselected members when an older batch finishes last', async () => {
    setSkills([skill({ name: 'alpha' }), skill({ name: 'beta' })]);
    const batch = deferred<UpdateCheckResponse>();
    mocks.checkUpdates.mockReturnValueOnce(batch.promise).mockResolvedValueOnce(response([updateInfo('alpha', { hasUpdate: true, status: 'updateAvailable' })], { checkedAtEpochMs: 200 }));
    const first = force(['alpha', 'beta']);
    await Promise.resolve();
    await force(['alpha']);
    batch.resolve(response([updateInfo('alpha'), updateInfo('beta')], { checkedAtEpochMs: 100 }));
    await first;
    expect(snapshot().skills.map((skill) => [skill.name, skill.hasUpdate])).toEqual([['alpha', true], ['beta', false]]);
  });

  it('keeps a confirmed comparison with the current structured failure', async () => {
    setSkills([skill()]);
    await force();
    const error = { kind: 'io' as const, data: { message: 'permission denied' } };
    mocks.checkUpdates.mockResolvedValue({ ...response([updateInfo('toolkit', { status: 'cannotCheck', freshness: 'unavailable', reason: 'upstreamUnavailable', error })]), outcome: 'notCompleted' });
    await force();
    expect(snapshot().skills[0]).toMatchObject({ hasUpdate: false, updateStatus: 'upToDate', updateError: error, updateAttempt: { outcome: 'notCompleted' } });
    mocks.listSkills.mockResolvedValue({ skills: [skill({ hasUpdate: false, updateStatus: null })], agents: [], pathExists: true });
    await useSkillsDataStore.getState().refreshContext(context);
    expect(snapshot().skills[0]?.updateError).toEqual(error);
    expect(snapshot().skills[0]?.updateStatus).toBe('upToDate');
  });

  it('invalidates the old response even when reinstalling the same version', async () => {
    setSkills([skill()]);
    const old = deferred<UpdateCheckResponse>();
    mocks.checkUpdates.mockReturnValue(old.promise);
    const checking = force();
    await Promise.resolve();
    mocks.listSkills.mockResolvedValue({ skills: [skill({ hasUpdate: false, updateStatus: null })], agents: [], pathExists: true });
    await useSkillsDataStore.getState().refreshContext(context, { origin: 'selfMutation', mutatedSkillNames: ['toolkit'], invalidateUpdates: true });
    old.resolve(response([updateInfo('toolkit', { hasUpdate: true, status: 'updateAvailable' })]));
    await checking;
    expect(snapshot().skills[0]?.hasUpdate).toBe(false);
  });

  it('does not restore an invalidated location from a late response', async () => {
    setSkills([skill()]);
    const old = deferred<UpdateCheckResponse>();
    mocks.checkUpdates.mockReturnValue(old.promise);
    const checking = force();
    await Promise.resolve();
    useSkillsDataStore.getState().invalidateContexts([context]);
    old.resolve(response([updateInfo('toolkit')]));
    await checking;
    expect(useSkillsDataStore.getState().snapshots[contextKey(context)]).toBeUndefined();
  });

  it('keeps newer evidence when a later request returns an older cached observation', async () => {
    setSkills([skill()]);
    mocks.checkUpdates.mockResolvedValueOnce(response([updateInfo('toolkit', { hasUpdate: true, status: 'updateAvailable' })], { checkedAtEpochMs: 200 }))
      .mockResolvedValueOnce(response([updateInfo('toolkit', { freshness: 'cached' })], { checkedAtEpochMs: 100 }));
    await force();
    await force();
    expect(snapshot().skills[0]?.hasUpdate).toBe(true);
  });

  it('does not extend one member freshness when another member of the source is checked', async () => {
    const rows = [skill({ name: 'alpha' }), skill({ name: 'beta' })];
    setSkills(rows);
    mocks.checkUpdates.mockResolvedValueOnce(response([updateInfo('alpha')], { checkedAtEpochMs: 100, expiresAtEpochMs: 1000 }))
      .mockResolvedValueOnce(response([updateInfo('beta')], { checkedAtEpochMs: 200, expiresAtEpochMs: 2000 }));
    await force(['alpha']);
    await force(['beta']);
    mocks.listSkills.mockResolvedValue({ skills: rows, agents: [], pathExists: true });
    await useSkillsDataStore.getState().refreshContext(context);
    expect(snapshot().skills.find((skill) => skill.name === 'alpha')?.updateEvidence?.expiresAtEpochMs).toBe(1000);
    expect(snapshot().skills.find((skill) => skill.name === 'beta')?.updateEvidence?.expiresAtEpochMs).toBe(2000);
  });

  it('ignores an old IPC rejection after the selected member succeeds in a newer request', async () => {
    setSkills([skill()]);
    const old = deferred<UpdateCheckResponse>();
    mocks.checkUpdates.mockReturnValueOnce(old.promise).mockResolvedValueOnce(response([updateInfo('toolkit')]));
    const first = useSkillsDataStore.getState().activateAutomaticChecks(context);
    await Promise.resolve();
    await force();
    old.reject({ kind: 'io', data: { message: 'old failure' } });
    await first;
    expect(snapshot().updateCheck?.error).toBeFalsy();
  });

  it('restores checking after Agent projections invalidate a snapshot', async () => {
    setSkills([skill()]);
    await useSkillsDataStore.getState().activateAutomaticChecks(context);
    useSkillsDataStore.getState().invalidateAgentProjections();
    mocks.listSkills.mockResolvedValue({ skills: [skill()], agents: [], pathExists: true });
    await useSkillsDataStore.getState().refreshContext(context);
    expect(mocks.checkUpdates).toHaveBeenCalledTimes(2);
    expect(snapshot().updateCheck?.results).toHaveLength(1);
  });

  it('clears successful results before a post-update refresh failure', async () => {
    setSkills([skill({ updateReason: 'missingRemoteHash' })]);
    mocks.listSkills.mockRejectedValue({ kind: 'io', data: { message: 'busy' } });
    await useSkillsDataStore.getState().applyUpdateResult(context, updateResponse('succeeded'));
    expect(snapshot().skills[0]).toMatchObject({ hasUpdate: false, updateReason: null });
    expect(snapshot().error?.kind).toBe('io');
  });

  it('keeps failed update comparisons during refresh', async () => {
    setSkills([skill()]);
    mocks.listSkills.mockResolvedValue({
      skills: [skill({ hasUpdate: false, updateStatus: null })],
      agents: [],
      pathExists: true,
    });

    await useSkillsDataStore.getState().applyUpdateResult(context, updateResponse('failed'));

    expect(mocks.listSkills).toHaveBeenCalledExactlyOnceWith(context);
    expect(snapshot().skills[0]).toMatchObject({
      hasUpdate: true,
      updateStatus: 'updateAvailable',
    });
  });

  it('does not automatically retry failed IPC in a settling loop', async () => {
    setSkills([skill()]);
    mocks.checkUpdates.mockRejectedValue({ kind: 'io', data: { message: 'offline' } });
    await useSkillsDataStore.getState().activateAutomaticChecks(context);
    await useSkillsDataStore.getState().reconcileAutomaticChecks(context);
    expect(mocks.checkUpdates).toHaveBeenCalledOnce();
    expect(snapshot().updateCheck?.error?.kind).toBe('io');
    expect(mocks.toastError).not.toHaveBeenCalled();
  });

  it.each(['sourceSessionCapacity', 'sourceDirectoryLinks'])('waits for manual retry after %s', async (capability) => {
    setSkills([skill()]);
    mocks.checkUpdates.mockResolvedValue(response([updateInfo('toolkit', {
      status: 'cannotCheck', hasUpdate: false, error: { kind: 'capabilityUnavailable', data: { capability, path: null } },
    })]));
    await useSkillsDataStore.getState().activateAutomaticChecks(context);
    await useSkillsDataStore.getState().reconcileAutomaticChecks(context);
    expect(mocks.checkUpdates).toHaveBeenCalledOnce();
    await force();
    expect(mocks.checkUpdates).toHaveBeenCalledTimes(2);
  });

  it('formats a manual IPC error without object stringification', async () => {
    setSkills([skill()]);
    mocks.checkUpdates.mockRejectedValue({ kind: 'io', data: { message: 'offline' } });
    await force();
    expect(mocks.toastError).toHaveBeenCalledOnce();
    expect(mocks.toastError.mock.calls[0]?.[0]).not.toContain('[object Object]');
  });

});
