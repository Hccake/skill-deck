import { beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  ContextRef,
  MutationUnitResult,
  SkillUpdateInfo,
  SourceUpdateCheckInfo,
  UpdateCheckResponse,
  UpdateCheckSelection,
  UpdateResponse,
} from '@/bindings';
import { contextKey } from '@/lib/context';
import { useSkillsDataStore } from '../skills-data';
import { mergeUpdateInfo, updateInfoCache, type SkillListItem } from '../skills-utils';

const mocks = vi.hoisted(() => ({
  listSkills: vi.fn(),
  listAgents: vi.fn(),
  checkUpdates: vi.fn(),
  checkSkillAudit: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  listSkills: (...args: unknown[]) => mocks.listSkills(...args),
  listAgents: (...args: unknown[]) => mocks.listAgents(...args),
  checkUpdates: (...args: unknown[]) => mocks.checkUpdates(...args),
  checkSkillAudit: (...args: unknown[]) => mocks.checkSkillAudit(...args),
}));

vi.mock('sonner', () => ({ toast: { error: vi.fn() } }));

const context: ContextRef = {
  environment: { kind: 'host' },
  scope: { scope: 'global' },
};

function skill(overrides: Partial<SkillListItem> = {}): SkillListItem {
  return {
    name: 'toolkit', description: '', path: '/skills/toolkit', canonicalPath: '/canonical/toolkit',
    scope: 'global', agents: ['codex'], associatedAgents: ['codex'], source: 'owner/repo', hasUpdate: true,
    canRunUpdate: true, canCheckForUpdates: true, updateStatus: 'updateAvailable',
    updateReason: null, ...overrides,
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
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
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
    ...overrides,
  };
}

function sourceInfo(
  source: string,
  overrides: Partial<SourceUpdateCheckInfo> = {},
): SourceUpdateCheckInfo {
  return {
    source,
    requestedRef: 'main',
    resolvedRef: 'main',
    refRevision: 'revision-1',
    checkedAtEpochMs: 100,
    expiresAtEpochMs: 200,
    freshness: 'fresh',
    lastAttempt: null,
    ...overrides,
  };
}

describe('skills data store', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    updateInfoCache.clear();
    useSkillsDataStore.setState({ snapshots: {}, auditCache: {}, isSyncing: false, checkingUpdateScopes: new Set() });
    mocks.listSkills.mockResolvedValue({ skills: [], agents: [], pathExists: true });
    mocks.listAgents.mockResolvedValue({ agents: [] });
    mocks.checkUpdates.mockResolvedValue({ sources: [], skills: [] });
  });

  it('merges typed update information into matching Skills', () => {
    const info: SkillUpdateInfo = {
      name: 'toolkit', source: 'owner/repo', hasUpdate: false, status: 'cannotCheck',
      capability: { canRunUpdate: true, canCheckForUpdates: false, reason: 'missingRemoteHash' },
      reason: 'missingRemoteHash', gitRef: null, sourceUrl: null, skillPath: null, freshness: 'fresh',
    };
    expect(mergeUpdateInfo([skill()], [info])[0]).toMatchObject({
      updateStatus: 'cannotCheck', updateReason: 'missingRemoteHash',
    });
  });

  it('delegates automatic freshness decisions to the Backend', async () => {
    setSkills([skill()]);
    await useSkillsDataStore.getState().syncUpdates(context);
    expect(mocks.checkUpdates).toHaveBeenCalledWith({ context, mode: 'automatic', selection: { kind: 'all' } });
  });

  it('checks only the selected project Context automatically', async () => {
    const projectContext: ContextRef = {
      environment: context.environment,
      scope: { scope: 'project', project_id: 'project-a' },
    };
    await useSkillsDataStore.getState().syncUpdates(projectContext);
    expect(mocks.checkUpdates).toHaveBeenCalledTimes(1);
    expect(mocks.checkUpdates).toHaveBeenCalledWith({
      context: projectContext, mode: 'automatic', selection: { kind: 'all' },
    });
  });

  it('forwards the typed force-check selection unchanged', async () => {
    const selection: UpdateCheckSelection = {
      kind: 'skills', skills: [{ context, skillName: 'toolkit' }],
    };
    await useSkillsDataStore.getState().forceCheckUpdates(context, selection);
    expect(mocks.checkUpdates).toHaveBeenCalledWith({ context, mode: 'force', selection });
  });

  it('keeps a newer Force(single) result when an older Automatic(all) finishes last', async () => {
    setSkills([skill({ hasUpdate: false, updateStatus: 'upToDate' })]);
    const automatic = deferred<UpdateCheckResponse>();
    const force = deferred<UpdateCheckResponse>();
    mocks.checkUpdates
      .mockImplementationOnce(() => automatic.promise)
      .mockImplementationOnce(() => force.promise);

    const automaticRequest = useSkillsDataStore.getState().syncUpdates(context);
    const forceRequest = useSkillsDataStore.getState().forceCheckUpdates(context, {
      kind: 'skills',
      skills: [{ context, skillName: 'toolkit' }],
    });

    force.resolve({
      sources: [sourceInfo('github.com/force/repo')],
      skills: [updateInfo('toolkit', {
        source: 'owner/repo',
        hasUpdate: true,
        status: 'updateAvailable',
        freshness: 'fresh',
      })],
    });
    await forceRequest;
    automatic.resolve({
      sources: [sourceInfo('github.com/automatic/repo', { freshness: 'stale' })],
      skills: [updateInfo('toolkit', {
        source: 'owner/repo',
        hasUpdate: false,
        status: 'upToDate',
        freshness: 'stale',
      })],
    });
    await automaticRequest;

    expect(useSkillsDataStore.getState().snapshots[contextKey(context)]).toMatchObject({
      skills: [{ name: 'toolkit', hasUpdate: true, updateFreshness: 'fresh' }],
      updateCheck: {
        sources: [{ source: 'github.com/force/repo' }],
        skillFreshness: { toolkit: 'fresh' },
      },
    });
  });

  it('keeps a Context marked checking until every request for it settles', async () => {
    const automatic = deferred<UpdateCheckResponse>();
    const force = deferred<UpdateCheckResponse>();
    mocks.checkUpdates
      .mockImplementationOnce(() => automatic.promise)
      .mockImplementationOnce(() => force.promise);

    const automaticRequest = useSkillsDataStore.getState().syncUpdates(context);
    const forceRequest = useSkillsDataStore.getState().forceCheckUpdates(context, { kind: 'all' });
    expect(useSkillsDataStore.getState().checkingUpdateScopes.has(contextKey(context))).toBe(true);

    automatic.resolve({ sources: [], skills: [] });
    await automaticRequest;
    expect(useSkillsDataStore.getState().checkingUpdateScopes.has(contextKey(context))).toBe(true);

    force.resolve({ sources: [], skills: [] });
    await forceRequest;
    expect(useSkillsDataStore.getState().checkingUpdateScopes.has(contextKey(context))).toBe(false);
  });

  it('preserves unselected source diagnostics and freshness after a partial Force check', async () => {
    setSkills([
      skill({ name: 'toolkit', source: 'toolkit/repo' }),
      skill({ name: 'reviewer', source: 'reviewer/repo', path: '/skills/reviewer', canonicalPath: '/canonical/reviewer' }),
    ]);
    mocks.checkUpdates.mockResolvedValueOnce({
      sources: [
        sourceInfo('github.com/toolkit/repo', { freshness: 'cached' }),
        sourceInfo('github.com/reviewer/repo', { freshness: 'stale' }),
      ],
      skills: [
        updateInfo('toolkit', { freshness: 'cached' }),
        updateInfo('reviewer', { freshness: 'stale' }),
      ],
    });
    await useSkillsDataStore.getState().forceCheckUpdates(context, { kind: 'all' });

    mocks.checkUpdates.mockResolvedValueOnce({
      sources: [sourceInfo('github.com/toolkit/repo', {
        freshness: 'coolingDown',
        lastAttempt: {
          checkedAtEpochMs: 300,
          failure: {
            reason: 'rateLimited',
            message: 'rate limited',
            retryAtEpochMs: 500,
            providerCooldown: true,
          },
        },
      })],
      skills: [updateInfo('toolkit', { freshness: 'coolingDown' })],
    });
    await useSkillsDataStore.getState().forceCheckUpdates(context, {
      kind: 'skills',
      skills: [{ context, skillName: 'toolkit' }],
    });

    const snapshot = useSkillsDataStore.getState().snapshots[contextKey(context)];
    expect(snapshot).toMatchObject({
      skills: [
        { name: 'reviewer', updateFreshness: 'stale' },
        { name: 'toolkit', updateFreshness: 'coolingDown' },
      ],
      updateCheck: {
        skillFreshness: { reviewer: 'stale', toolkit: 'coolingDown' },
      },
    });
    expect(snapshot?.updateCheck?.sources).toEqual(expect.arrayContaining([
      expect.objectContaining({ source: 'github.com/reviewer/repo', freshness: 'stale' }),
      expect.objectContaining({
        source: 'github.com/toolkit/repo',
        freshness: 'coolingDown',
        lastAttempt: expect.objectContaining({
          failure: expect.objectContaining({ retryAtEpochMs: 500, providerCooldown: true }),
        }),
      }),
    ]));
  });

  it('preserves unselected update state when applying and replaying a partial force check', async () => {
    const toolkit = skill({ name: 'toolkit', hasUpdate: false, updateStatus: 'upToDate' });
    const reviewer = skill({
      name: 'reviewer',
      path: '/skills/reviewer',
      canonicalPath: '/canonical/reviewer',
      hasUpdate: true,
      updateStatus: 'cannotCheck',
      updateReason: 'rate-limited',
    });
    setSkills([toolkit, reviewer]);
    mocks.checkUpdates.mockResolvedValue({
      sources: [],
      skills: [{
        name: 'toolkit', source: 'owner/repo', hasUpdate: true, status: 'updateAvailable',
        capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
        reason: null, gitRef: null, sourceUrl: null, skillPath: null, freshness: 'fresh',
      } satisfies SkillUpdateInfo],
    });

    await useSkillsDataStore.getState().forceCheckUpdates(context, {
      kind: 'skills',
      skills: [{ context, skillName: 'toolkit' }],
    });

    expect(useSkillsDataStore.getState().snapshots[contextKey(context)]?.skills.find((item) => item.name === 'reviewer')).toMatchObject({
      hasUpdate: true,
      updateStatus: 'cannotCheck',
      updateReason: 'rate-limited',
    });

    mocks.listSkills.mockResolvedValue({
      skills: [
        skill({ name: 'toolkit', hasUpdate: false, updateStatus: null, updateReason: null }),
        skill({ name: 'reviewer', path: '/skills/reviewer', canonicalPath: '/canonical/reviewer', hasUpdate: false, updateStatus: null, updateReason: null }),
      ],
      agents: [],
      pathExists: true,
    });
    await useSkillsDataStore.getState().refreshContext(context);

    expect(useSkillsDataStore.getState().snapshots[contextKey(context)]?.skills.find((item) => item.name === 'reviewer')).toMatchObject({
      hasUpdate: true,
      updateStatus: 'cannotCheck',
      updateReason: 'rate-limited',
    });
  });

  it('applies succeeded workflow results to the matching Context snapshot', () => {
    setSkills([skill()]);
    (useSkillsDataStore.getState() as unknown as { applyUpdateResult: (context: ContextRef, result: UpdateResponse) => void })
      .applyUpdateResult(context, updateResponse('succeeded'));
    expect(useSkillsDataStore.getState().snapshots[contextKey(context)]?.skills[0]).toMatchObject({
      hasUpdate: false, updateStatus: 'upToDate', updateReason: null,
    });
  });

  it('keeps the update display when the workflow reports a failed unit', () => {
    setSkills([skill()]);
    (useSkillsDataStore.getState() as unknown as { applyUpdateResult: (context: ContextRef, result: UpdateResponse) => void })
      .applyUpdateResult(context, updateResponse('failed'));
    expect(useSkillsDataStore.getState().snapshots[contextKey(context)]?.skills[0]?.hasUpdate).toBe(true);
  });

  it('does not retain update preview, execution, result, or conflict authority', () => {
    const state = useSkillsDataStore.getState();
    for (const key of [
      'prepareUpdate', 'executePreparedUpdate', 'prepareRetryUpdate', 'closeUpdateDialog',
      'lastUpdatePreview', 'lastUpdateResponse', 'lastUpdateResults', 'preparedUpdateContext',
      'updateDialogOpen', 'updatingSkills',
    ]) expect(state).not.toHaveProperty(key);
  });
});
