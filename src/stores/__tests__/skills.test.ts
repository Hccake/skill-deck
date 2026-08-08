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
import { sourceDiagnosticsForEnvironment, useSkillsDataStore } from '../skills-data';
import { mergeUpdateInfo, updateInfoCache, type SkillListItem } from '../skills-utils';

const mocks = vi.hoisted(() => ({
  listSkills: vi.fn(),
  listAgents: vi.fn(),
  checkUpdates: vi.fn(),
  checkSkillAudit: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  listSkills: (...args: unknown[]) => mocks.listSkills(...args),
  listAgents: (...args: unknown[]) => mocks.listAgents(...args),
  checkUpdates: (...args: unknown[]) => mocks.checkUpdates(...args),
  checkSkillAudit: (...args: unknown[]) => mocks.checkSkillAudit(...args),
}));

vi.mock('sonner', () => ({ toast: { error: mocks.toastError } }));

const context: SkillLocationRef = {
  environment: { kind: 'native' },
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
    requestedRef: 'HEAD',
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
    useSkillsDataStore.setState({
      snapshots: {},
      auditCache: {},
      updateCheckSessions: {},
      isSyncing: false,
      checkingUpdateScopes: new Set(),
      automaticUpdateScopes: new Set(),
      forceUpdateScopes: new Set(),
    });
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

  it('binds a default-ref Skill only to HEAD source evidence', () => {
    const headEvidence = sourceInfo('github.com/owner/repo');
    const releaseEvidence = sourceInfo('github.com/owner/repo', {
      requestedRef: 'release',
      freshness: 'coolingDown',
    });
    const [merged] = mergeUpdateInfo(
      [skill({ gitRef: null, skillPath: 'skills/toolkit' })],
      [updateInfo('toolkit', {
        source: 'owner/repo',
        gitRef: null,
        skillPath: 'skills/toolkit',
        status: 'cannotCheck',
        reason: 'upstreamUnavailable',
      })],
      { sources: [releaseEvidence, headEvidence] },
    );

    expect(merged.updateEvidence?.requestedRef).toBe('HEAD');
  });

  it('delegates automatic freshness decisions to the Backend', async () => {
    setSkills([skill()]);
    await useSkillsDataStore.getState().syncUpdates(context);
    expect(mocks.checkUpdates).toHaveBeenCalledWith({ context, mode: 'automatic', selection: { kind: 'all' } });
  });

  it('admits an automatic check only after an eligible snapshot and only once per session', async () => {
    const eligible = skill({ skillPath: 'skills/toolkit' });
    useSkillsDataStore.setState({
      snapshots: {
        [contextKey(context)]: {
          skills: [eligible], agents: [], pathExists: true, loading: false, error: null, requestId: 1,
        },
      },
    });
    mocks.checkUpdates.mockResolvedValue({ outcome: 'completed', sources: [], skills: [] });

    await useSkillsDataStore.getState().activateAutomaticChecks(context);
    await useSkillsDataStore.getState().activateAutomaticChecks(context);

    expect(mocks.checkUpdates).toHaveBeenCalledTimes(1);
    expect(mocks.checkUpdates).toHaveBeenCalledWith({
      context,
      mode: 'automatic',
      selection: { kind: 'all' },
    });
  });

  it('waits for the first eligible Skill before marking the initial attempt', async () => {
    setSkills([]);
    await useSkillsDataStore.getState().activateAutomaticChecks(context);

    expect(mocks.checkUpdates).not.toHaveBeenCalled();
    expect(useSkillsDataStore.getState().updateCheckSessions[contextKey(context)]?.initialAttempted)
      .toBe(false);

    mocks.listSkills.mockResolvedValue({
      skills: [skill({ skillPath: 'skills/toolkit' })],
      agents: [],
      pathExists: true,
    });
    mocks.checkUpdates.mockResolvedValue({ outcome: 'completed', sources: [], skills: [] });
    await useSkillsDataStore.getState().refreshContext(context);

    expect(mocks.checkUpdates).toHaveBeenCalledTimes(1);
    expect(mocks.checkUpdates).toHaveBeenCalledWith({
      context,
      mode: 'automatic',
      selection: { kind: 'all' },
    });
  });

  it('waits when every Skill is ineligible and checks when one becomes eligible', async () => {
    setSkills([skill({
      canCheckForUpdates: false,
      updateStatus: 'cannotCheck',
      updateReason: 'missingRemoteHash',
      skillPath: null,
    })]);
    await useSkillsDataStore.getState().activateAutomaticChecks(context);

    expect(mocks.checkUpdates).not.toHaveBeenCalled();
    expect(useSkillsDataStore.getState().updateCheckSessions[contextKey(context)]?.initialAttempted)
      .toBe(false);

    mocks.listSkills.mockResolvedValue({
      skills: [skill({ skillPath: 'skills/toolkit' })],
      agents: [],
      pathExists: true,
    });
    mocks.checkUpdates.mockResolvedValue({ outcome: 'completed', sources: [], skills: [] });
    await useSkillsDataStore.getState().refreshContext(context);

    expect(mocks.checkUpdates).toHaveBeenCalledTimes(1);
    expect(mocks.checkUpdates).toHaveBeenCalledWith({
      context,
      mode: 'automatic',
      selection: { kind: 'all' },
    });
  });

  it('checks each Context once and does not repeat when revisiting either Context', async () => {
    const ubuntuContext: SkillLocationRef = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    };
    useSkillsDataStore.setState({
      snapshots: {
        [contextKey(context)]: {
          skills: [skill({ skillPath: 'skills/toolkit' })],
          agents: [], pathExists: true, loading: false, error: null, requestId: 1,
        },
        [contextKey(ubuntuContext)]: {
          skills: [skill({
            name: 'reviewer',
            path: '/skills/reviewer',
            canonicalPath: '/canonical/reviewer',
            skillPath: 'skills/reviewer',
          })],
          agents: [], pathExists: true, loading: false, error: null, requestId: 1,
        },
      },
    });
    mocks.checkUpdates.mockResolvedValue({ outcome: 'completed', sources: [], skills: [] });

    await useSkillsDataStore.getState().activateAutomaticChecks(context);
    await useSkillsDataStore.getState().activateAutomaticChecks(ubuntuContext);
    await useSkillsDataStore.getState().activateAutomaticChecks(context);
    await useSkillsDataStore.getState().activateAutomaticChecks(ubuntuContext);

    expect(mocks.checkUpdates).toHaveBeenCalledTimes(2);
    expect(mocks.checkUpdates).toHaveBeenNthCalledWith(1, {
      context,
      mode: 'automatic',
      selection: { kind: 'all' },
    });
    expect(mocks.checkUpdates).toHaveBeenNthCalledWith(2, {
      context: ubuntuContext,
      mode: 'automatic',
      selection: { kind: 'all' },
    });
  });

  it('checks Global and each Project once across Context round trips', async () => {
    const projectA: SkillLocationRef = {
      environment: context.environment,
      scope: { scope: 'project', project_id: 'project-a' },
    };
    const projectB: SkillLocationRef = {
      environment: context.environment,
      scope: { scope: 'project', project_id: 'project-b' },
    };
    useSkillsDataStore.setState({
      snapshots: {
        [contextKey(context)]: {
          skills: [skill({ skillPath: 'skills/toolkit' })],
          agents: [], pathExists: true, loading: false, error: null, requestId: 1,
        },
        [contextKey(projectA)]: {
          skills: [skill({ scope: 'project', skillPath: 'skills/toolkit' })],
          agents: [], pathExists: true, loading: false, error: null, requestId: 1,
        },
        [contextKey(projectB)]: {
          skills: [skill({ scope: 'project', skillPath: 'skills/toolkit' })],
          agents: [], pathExists: true, loading: false, error: null, requestId: 1,
        },
      },
    });
    mocks.checkUpdates.mockResolvedValue({ outcome: 'completed', sources: [], skills: [] });

    await useSkillsDataStore.getState().activateAutomaticChecks(context);
    await useSkillsDataStore.getState().activateAutomaticChecks(projectA);
    await useSkillsDataStore.getState().activateAutomaticChecks(projectB);
    await useSkillsDataStore.getState().activateAutomaticChecks(context);
    await useSkillsDataStore.getState().activateAutomaticChecks(projectB);
    await useSkillsDataStore.getState().activateAutomaticChecks(projectA);

    expect(mocks.checkUpdates).toHaveBeenCalledTimes(3);
    expect(mocks.checkUpdates).toHaveBeenNthCalledWith(1, {
      context,
      mode: 'automatic',
      selection: { kind: 'all' },
    });
    expect(mocks.checkUpdates).toHaveBeenNthCalledWith(2, {
      context: projectA,
      mode: 'automatic',
      selection: { kind: 'all' },
    });
    expect(mocks.checkUpdates).toHaveBeenNthCalledWith(3, {
      context: projectB,
      mode: 'automatic',
      selection: { kind: 'all' },
    });
  });

  it('keeps the session gate when the same Environment reconnects and reloads its snapshot', async () => {
    const installed = skill({ skillPath: 'skills/toolkit' });
    setSkills([installed]);
    mocks.checkUpdates.mockResolvedValue({ outcome: 'completed', sources: [], skills: [] });
    await useSkillsDataStore.getState().activateAutomaticChecks(context);

    useSkillsDataStore.getState().invalidateContexts([context]);
    mocks.listSkills.mockResolvedValue({ skills: [installed], agents: [], pathExists: true });
    await useSkillsDataStore.getState().refreshContext(context);
    await useSkillsDataStore.getState().activateAutomaticChecks(context);

    expect(mocks.checkUpdates).toHaveBeenCalledTimes(1);
    expect(useSkillsDataStore.getState().updateCheckSessions[contextKey(context)]).toMatchObject({
      active: true,
      initialAttempted: true,
    });
  });

  it('checks only a newly observed or changed skill with a targeted automatic selection', async () => {
    const first = skill({ skillPath: 'skills/toolkit' });
    setSkills([first]);
    mocks.checkUpdates.mockResolvedValue({ outcome: 'completed', sources: [], skills: [] });
    await useSkillsDataStore.getState().activateAutomaticChecks(context);

    mocks.listSkills.mockResolvedValue({
      skills: [
        first,
        skill({ name: 'reviewer', path: '/skills/reviewer', canonicalPath: '/canonical/reviewer', skillPath: 'skills/reviewer' }),
      ],
      agents: [],
      pathExists: true,
    });
    await useSkillsDataStore.getState().refreshContext(context);

    expect(mocks.checkUpdates).toHaveBeenCalledTimes(2);
    expect(mocks.checkUpdates).toHaveBeenLastCalledWith({
      context,
      mode: 'automatic',
      selection: {
        kind: 'skills',
        skills: [{ context, skillName: 'reviewer' }],
      },
    });
  });

  it('checks a skill again when it disappears and later reappears', async () => {
    const first = skill({ skillPath: 'skills/toolkit' });
    setSkills([first]);
    mocks.checkUpdates.mockResolvedValue({ outcome: 'completed', sources: [], skills: [] });
    await useSkillsDataStore.getState().activateAutomaticChecks(context);

    mocks.listSkills.mockResolvedValue({ skills: [], agents: [], pathExists: true });
    await useSkillsDataStore.getState().refreshContext(context);

    mocks.listSkills.mockResolvedValue({
      skills: [first],
      agents: [],
      pathExists: true,
    });
    await useSkillsDataStore.getState().refreshContext(context);

    expect(mocks.checkUpdates).toHaveBeenCalledTimes(2);
    expect(mocks.checkUpdates).toHaveBeenLastCalledWith({
      context,
      mode: 'automatic',
      selection: {
        kind: 'skills',
        skills: [{ context, skillName: 'toolkit' }],
      },
    });
  });

  it('targets a Skill whose source identity changes during a passive refresh', async () => {
    const first = skill({ gitRef: 'main', skillPath: 'skills/toolkit' });
    setSkills([first]);
    mocks.checkUpdates.mockResolvedValueOnce({
      outcome: 'completed',
      sources: [sourceInfo('github.com/owner/repo')],
      skills: [updateInfo('toolkit', {
        source: 'owner/repo',
        gitRef: 'main',
        skillPath: 'skills/toolkit',
      })],
    });
    await useSkillsDataStore.getState().activateAutomaticChecks(context);

    mocks.listSkills.mockResolvedValue({
      skills: [skill({ source: 'owner/next-repo', gitRef: 'release', skillPath: 'packages/toolkit' })],
      agents: [],
      pathExists: true,
    });
    mocks.checkUpdates.mockResolvedValueOnce({
      outcome: 'completed',
      sources: [sourceInfo('github.com/owner/next-repo', { requestedRef: 'release' })],
      skills: [updateInfo('toolkit', {
        source: 'owner/next-repo',
        gitRef: 'release',
        skillPath: 'packages/toolkit',
      })],
    });
    await useSkillsDataStore.getState().refreshContext(context);

    expect(mocks.checkUpdates).toHaveBeenCalledTimes(2);
    expect(mocks.checkUpdates).toHaveBeenLastCalledWith({
      context,
      mode: 'automatic',
      selection: {
        kind: 'skills',
        skills: [{ context, skillName: 'toolkit' }],
      },
    });
    expect(updateInfoCache.get(contextKey(context))?.sources).toEqual([
      expect.objectContaining({ source: 'github.com/owner/next-repo', requestedRef: 'release' }),
    ]);
  });

  it('preserves unselected results when a targeted Automatic response is partial', async () => {
    const toolkit = skill({ name: 'toolkit', source: 'toolkit/repo', gitRef: 'main', skillPath: 'skills/toolkit' });
    const reviewer = skill({
      name: 'reviewer',
      path: '/skills/reviewer',
      canonicalPath: '/canonical/reviewer',
      skillPath: 'skills/reviewer',
      hasUpdate: false,
      updateStatus: 'upToDate',
    });
    setSkills([toolkit, reviewer]);
    mocks.checkUpdates.mockResolvedValueOnce({
      outcome: 'completed',
      sources: [sourceInfo('github.com/toolkit/repo'), sourceInfo('github.com/reviewer/repo')],
      skills: [
        updateInfo('toolkit', { source: 'toolkit/repo', hasUpdate: false }),
        updateInfo('reviewer', { source: 'reviewer/repo', hasUpdate: false }),
      ],
    });
    await useSkillsDataStore.getState().activateAutomaticChecks(context);

    mocks.listSkills.mockResolvedValue({
      skills: [
        skill({ name: 'toolkit', source: 'toolkit/repo', gitRef: 'release', skillPath: 'skills/toolkit' }),
        reviewer,
      ],
      agents: [],
      pathExists: true,
    });
    mocks.checkUpdates.mockResolvedValueOnce({
      outcome: 'completed',
      sources: [sourceInfo('github.com/toolkit/repo', { refRevision: 'revision-2' })],
      skills: [updateInfo('toolkit', {
        source: 'toolkit/repo',
        hasUpdate: true,
        status: 'updateAvailable',
        gitRef: 'release',
      })],
    });
    await useSkillsDataStore.getState().refreshContext(context);

    expect(mocks.checkUpdates).toHaveBeenLastCalledWith({
      context,
      mode: 'automatic',
      selection: { kind: 'skills', skills: [{ context, skillName: 'toolkit' }] },
    });
    expect(updateInfoCache.get(contextKey(context))?.results.map((item) => item.name))
      .toEqual(['reviewer', 'toolkit']);
    expect(useSkillsDataStore.getState().snapshots[contextKey(context)]?.skills).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ name: 'toolkit', updateStatus: 'updateAvailable', hasUpdate: true }),
        expect.objectContaining({ name: 'reviewer', updateStatus: 'upToDate', hasUpdate: false }),
      ]),
    );
  });

  it('records a self-mutation identity without scheduling a targeted Automatic check', async () => {
    setSkills([skill({ gitRef: 'main', skillPath: 'skills/toolkit' })]);
    mocks.checkUpdates.mockResolvedValue({ outcome: 'completed', sources: [], skills: [] });
    await useSkillsDataStore.getState().activateAutomaticChecks(context);

    mocks.listSkills.mockResolvedValue({
      skills: [skill({ source: 'owner/repaired', gitRef: 'release', skillPath: 'packages/toolkit' })],
      agents: [],
      pathExists: true,
    });
    await useSkillsDataStore.getState().refreshContext(context, {
      origin: 'selfMutation',
      mutatedSkillNames: ['toolkit'],
    });
    await useSkillsDataStore.getState().refreshContext(context);

    expect(mocks.checkUpdates).toHaveBeenCalledTimes(1);
  });

  it('targets an unrelated external change observed during a self-mutation refresh', async () => {
    const toolkit = skill({ name: 'toolkit', gitRef: 'main', skillPath: 'skills/toolkit' });
    const reviewer = skill({
      name: 'reviewer',
      path: '/skills/reviewer',
      canonicalPath: '/canonical/reviewer',
      source: 'reviewer/repo',
      gitRef: 'main',
      skillPath: 'skills/reviewer',
    });
    setSkills([toolkit, reviewer]);
    mocks.checkUpdates.mockResolvedValue({ outcome: 'completed', sources: [], skills: [] });
    await useSkillsDataStore.getState().activateAutomaticChecks(context);
    mocks.checkUpdates.mockClear();

    mocks.listSkills.mockResolvedValueOnce({
      skills: [
        skill({ name: 'toolkit', source: 'toolkit/repaired', gitRef: 'release', skillPath: 'packages/toolkit' }),
        skill({
          name: 'reviewer',
          path: '/skills/reviewer',
          canonicalPath: '/canonical/reviewer',
          source: 'reviewer/external',
          gitRef: 'release',
          skillPath: 'packages/reviewer',
        }),
      ],
      agents: [],
      pathExists: true,
    });
    await useSkillsDataStore.getState().refreshContext(context, {
      origin: 'selfMutation',
      mutatedSkillNames: ['toolkit'],
    });

    expect(mocks.checkUpdates).toHaveBeenCalledTimes(1);
    expect(mocks.checkUpdates).toHaveBeenCalledWith({
      context,
      mode: 'automatic',
      selection: { kind: 'skills', skills: [{ context, skillName: 'reviewer' }] },
    });
  });

  it('records a self-mutation before the Context is first activated', async () => {
    const installed = skill({ gitRef: 'main', skillPath: 'skills/toolkit' });
    const existing = skill({
      name: 'reviewer',
      path: '/skills/reviewer',
      canonicalPath: '/canonical/reviewer',
      source: 'reviewer/repo',
      gitRef: 'main',
      skillPath: 'skills/reviewer',
    });
    mocks.listSkills.mockResolvedValueOnce({
      skills: [installed, existing],
      agents: [],
      pathExists: true,
    });
    mocks.checkUpdates.mockResolvedValueOnce({ outcome: 'completed', sources: [], skills: [] });

    await useSkillsDataStore.getState().refreshContext(context, {
      origin: 'selfMutation',
      mutatedSkillNames: ['toolkit'],
    });
    await useSkillsDataStore.getState().activateAutomaticChecks(context);

    expect(mocks.checkUpdates).toHaveBeenCalledWith({
      context,
      mode: 'automatic',
      selection: { kind: 'skills', skills: [{ context, skillName: 'reviewer' }] },
    });
    expect(useSkillsDataStore.getState().updateCheckSessions[contextKey(context)]).toMatchObject({
      active: true,
      initialAttempted: true,
    });
  });

  it('prunes only the changed ref diagnostic when one repository has multiple refs', async () => {
    const head = skill({
      name: 'head-skill',
      path: '/skills/head-skill',
      canonicalPath: '/canonical/head-skill',
      source: 'owner/repo',
      gitRef: null,
      skillPath: 'skills/head',
    });
    const release = skill({
      name: 'release-skill',
      path: '/skills/release-skill',
      canonicalPath: '/canonical/release-skill',
      source: 'owner/repo',
      gitRef: 'release',
      skillPath: 'skills/release',
    });
    setSkills([head, release]);
    mocks.checkUpdates.mockResolvedValueOnce({
      outcome: 'completed',
      sources: [
        sourceInfo('github.com/owner/repo'),
        sourceInfo('github.com/owner/repo', { requestedRef: 'release' }),
      ],
      skills: [
        updateInfo('head-skill', { source: 'owner/repo', gitRef: null, skillPath: 'skills/head' }),
        updateInfo('release-skill', { source: 'owner/repo', gitRef: 'release', skillPath: 'skills/release' }),
      ],
    });
    await useSkillsDataStore.getState().activateAutomaticChecks(context);

    mocks.listSkills.mockResolvedValueOnce({
      skills: [
        head,
        { ...release, source: 'owner/next-repo' },
      ],
      agents: [],
      pathExists: true,
    });
    mocks.checkUpdates.mockResolvedValueOnce({
      outcome: 'completed',
      sources: [sourceInfo('github.com/owner/next-repo', { requestedRef: 'release' })],
      skills: [updateInfo('release-skill', {
        source: 'owner/next-repo',
        gitRef: 'release',
        skillPath: 'skills/release',
      })],
    });
    await useSkillsDataStore.getState().refreshContext(context);

    expect(updateInfoCache.get(contextKey(context))?.sources).toEqual([
      expect.objectContaining({ source: 'github.com/owner/repo', requestedRef: 'HEAD' }),
      expect.objectContaining({ source: 'github.com/owner/next-repo', requestedRef: 'release' }),
    ]);
  });

  it('clears only Native GitHub provider cooldown after credential maintenance succeeds', async () => {
    const nativeCooldown = sourceInfo('github.com/owner/repo', {
      freshness: 'coolingDown',
      lastAttempt: {
        checkedAtEpochMs: 100,
        failure: {
          reason: 'rateLimited',
          message: 'rate limited',
          retryAtEpochMs: 500,
          providerCooldown: true,
        },
      },
    });
    const wslContext: SkillLocationRef = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    };
    useSkillsDataStore.setState({
      snapshots: {
        [contextKey(context)]: {
          skills: [skill({ skillPath: 'skills/toolkit', updateEvidence: nativeCooldown })],
          updateCheck: { outcome: 'notCompleted', sources: [nativeCooldown], skillFreshness: {}, checkedAt: 100 },
          agents: [], pathExists: true, loading: false, error: null, requestId: 1,
        },
        [contextKey(wslContext)]: {
          skills: [skill({ updateEvidence: nativeCooldown })],
          updateCheck: { outcome: 'notCompleted', sources: [nativeCooldown], skillFreshness: {}, checkedAt: 100 },
          agents: [], pathExists: true, loading: false, error: null, requestId: 1,
        },
      },
    });
    updateInfoCache.set(contextKey(context), {
      results: [updateInfo('toolkit', {
        source: 'owner/repo',
        freshness: 'coolingDown',
      })],
      sources: [nativeCooldown],
      checkedAt: 100,
      completeness: 'complete',
      outcome: 'notCompleted',
    });
    mocks.listSkills.mockResolvedValueOnce({
      skills: [skill({ skillPath: 'skills/toolkit' })],
      agents: [],
      pathExists: true,
    });

    useSkillsDataStore.getState().clearNativeGithubProviderCooldown();
    await useSkillsDataStore.getState().refreshContext(context);

    expect(useSkillsDataStore.getState().snapshots[contextKey(context)]
      .skills[0].updateEvidence?.lastAttempt?.failure).toMatchObject({
      providerCooldown: false,
      retryAtEpochMs: null,
    });
    expect(useSkillsDataStore.getState().snapshots[contextKey(wslContext)]
      .skills[0].updateEvidence?.lastAttempt?.failure?.providerCooldown).toBe(true);
  });

  it('collects source diagnostics across Contexts in only the selected Environment', () => {
    const nativeProject: SkillLocationRef = {
      environment: { kind: 'native' },
      scope: { scope: 'project', project_id: 'project-a' },
    };
    const wslContext: SkillLocationRef = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    };
    const snapshot = (source: string) => ({
      skills: [],
      updateCheck: {
        outcome: 'completed' as const,
        sources: [sourceInfo(source)],
        skillFreshness: {},
        checkedAt: 100,
      },
      agents: [], pathExists: true, loading: false, error: null, requestId: 1,
    });
    const snapshots = {
      [contextKey(context)]: snapshot('github.com/owner/global'),
      [contextKey(nativeProject)]: snapshot('github.com/owner/project'),
      [contextKey(wslContext)]: snapshot('github.com/owner/wsl'),
    };

    expect(sourceDiagnosticsForEnvironment(snapshots, { kind: 'native' }).map((item) => item.source))
      .toEqual(['github.com/owner/global', 'github.com/owner/project']);
  });

  it('does not turn a self-mutation into a delayed initial Automatic check', async () => {
    setSkills([]);
    await useSkillsDataStore.getState().activateAutomaticChecks(context);
    expect(mocks.checkUpdates).not.toHaveBeenCalled();

    const installed = skill({ gitRef: 'main', skillPath: 'skills/toolkit' });
    mocks.listSkills.mockResolvedValue({
      skills: [installed],
      agents: [],
      pathExists: true,
    });
    await useSkillsDataStore.getState().refreshContext(context, {
      origin: 'selfMutation',
      mutatedSkillNames: ['toolkit'],
    });
    await useSkillsDataStore.getState().refreshContext(context, { origin: 'initial' });
    await useSkillsDataStore.getState().activateAutomaticChecks(context);

    expect(mocks.checkUpdates).not.toHaveBeenCalled();
    expect(useSkillsDataStore.getState().updateCheckSessions[contextKey(context)]).toMatchObject({
      active: true,
      initialAttempted: true,
    });
  });

  it('ignores an out-of-order self-mutation fingerprint after a newer refresh commits', async () => {
    const original = skill({ source: 'owner/original', gitRef: 'main', skillPath: 'skills/toolkit' });
    const repaired = skill({ source: 'owner/repaired', gitRef: 'release', skillPath: 'packages/toolkit' });
    setSkills([original]);
    await useSkillsDataStore.getState().activateAutomaticChecks(context);
    mocks.checkUpdates.mockClear();

    const olderRefresh = deferred<Awaited<ReturnType<typeof mocks.listSkills>>>();
    const newerRefresh = deferred<Awaited<ReturnType<typeof mocks.listSkills>>>();
    mocks.listSkills
      .mockReturnValueOnce(olderRefresh.promise)
      .mockReturnValueOnce(newerRefresh.promise);

    const olderRequest = useSkillsDataStore.getState().refreshContext(
      context,
      { origin: 'selfMutation', mutatedSkillNames: ['toolkit'] },
    );
    const newerRequest = useSkillsDataStore.getState().refreshContext(
      context,
      { origin: 'selfMutation', mutatedSkillNames: ['toolkit'] },
    );

    newerRefresh.resolve({ skills: [repaired], agents: [], pathExists: true });
    await newerRequest;
    olderRefresh.resolve({ skills: [original], agents: [], pathExists: true });
    await olderRequest;

    mocks.listSkills.mockResolvedValueOnce({ skills: [repaired], agents: [], pathExists: true });
    await useSkillsDataStore.getState().refreshContext(context, { origin: 'passive' });

    expect(mocks.checkUpdates).not.toHaveBeenCalled();
  });

  it('allows a passive toolbar sync to target a newly discovered Skill', async () => {
    setSkills([skill({ gitRef: 'main', skillPath: 'skills/toolkit' })]);
    mocks.checkUpdates.mockResolvedValue({ outcome: 'completed', sources: [], skills: [] });
    await useSkillsDataStore.getState().activateAutomaticChecks(context);
    mocks.checkUpdates.mockClear();
    mocks.listSkills.mockResolvedValueOnce({
      skills: [
        skill({ gitRef: 'main', skillPath: 'skills/toolkit' }),
        skill({
          name: 'reviewer',
          path: '/skills/reviewer',
          canonicalPath: '/canonical/reviewer',
          source: 'owner/repo',
          gitRef: 'main',
          skillPath: 'skills/reviewer',
        }),
      ],
      agents: [],
      pathExists: true,
    });

    await (useSkillsDataStore.getState() as unknown as {
      syncSkills: (context: SkillLocationRef, options: { origin: 'passive' }) => Promise<void>;
    }).syncSkills(context, { origin: 'passive' });

    expect(mocks.checkUpdates).toHaveBeenCalledWith({
      context,
      mode: 'automatic',
      selection: {
        kind: 'skills',
        skills: [{ context, skillName: 'reviewer' }],
      },
    });
  });

  it('reconciles a Skill discovered while Automatic is pending after that request settles', async () => {
    const first = skill({ skillPath: 'skills/toolkit' });
    setSkills([first]);
    const automatic = deferred<UpdateCheckResponse>();
    mocks.checkUpdates
      .mockReturnValueOnce(automatic.promise)
      .mockResolvedValue({ outcome: 'completed', sources: [], skills: [] });

    const automaticRequest = useSkillsDataStore.getState().activateAutomaticChecks(context);
    expect(mocks.checkUpdates).toHaveBeenCalledTimes(1);

    mocks.listSkills.mockResolvedValueOnce({
      skills: [
        first,
        skill({
          name: 'reviewer',
          path: '/skills/reviewer',
          canonicalPath: '/canonical/reviewer',
          skillPath: 'skills/reviewer',
        }),
      ],
      agents: [],
      pathExists: true,
    });
    await useSkillsDataStore.getState().refreshContext(context, { origin: 'passive' });
    expect(mocks.checkUpdates).toHaveBeenCalledTimes(1);

    automatic.resolve({ outcome: 'completed', sources: [], skills: [] });
    await automaticRequest;

    expect(mocks.checkUpdates).toHaveBeenCalledTimes(2);
    expect(mocks.checkUpdates).toHaveBeenLastCalledWith({
      context,
      mode: 'automatic',
      selection: {
        kind: 'skills',
        skills: [{ context, skillName: 'reviewer' }],
      },
    });
  });

  it('records an automatic attempt even when the IPC rejects and does not retry passively', async () => {
    setSkills([skill({ skillPath: 'skills/toolkit' })]);
    mocks.checkUpdates.mockRejectedValue(new Error('disconnected'));

    await useSkillsDataStore.getState().activateAutomaticChecks(context);
    await useSkillsDataStore.getState().activateAutomaticChecks(context);

    expect(mocks.checkUpdates).toHaveBeenCalledTimes(1);
    expect(mocks.toastError).toHaveBeenCalledTimes(1);
  });

  it('rejects a duplicate Force request before sending a second IPC call', async () => {
    const pending = deferred<UpdateCheckResponse>();
    mocks.checkUpdates.mockReturnValue(pending.promise);

    const first = useSkillsDataStore.getState().forceCheckUpdates(context, { kind: 'all' });
    const second = useSkillsDataStore.getState().forceCheckUpdates(context, { kind: 'all' });

    expect(mocks.checkUpdates).toHaveBeenCalledTimes(1);
    expect(await second).toBeNull();
    pending.resolve({ outcome: 'completed', sources: [], skills: [] });
    await first;
  });

  it('defers a passive Automatic check until the pending Force settles', async () => {
    const first = skill({ skillPath: 'skills/toolkit' });
    setSkills([first]);
    mocks.checkUpdates.mockResolvedValueOnce({ outcome: 'completed', sources: [], skills: [] });
    await useSkillsDataStore.getState().activateAutomaticChecks(context);

    const force = deferred<UpdateCheckResponse>();
    mocks.checkUpdates.mockImplementationOnce(() => force.promise);
    mocks.checkUpdates.mockResolvedValue({ outcome: 'completed', sources: [], skills: [] });
    const forceRequest = useSkillsDataStore.getState().forceCheckUpdates(context, { kind: 'all' });

    mocks.listSkills.mockResolvedValue({
      skills: [
        first,
        skill({ name: 'reviewer', path: '/skills/reviewer', canonicalPath: '/canonical/reviewer', skillPath: 'skills/reviewer' }),
      ],
      agents: [],
      pathExists: true,
    });
    await useSkillsDataStore.getState().refreshContext(context);

    expect(mocks.checkUpdates).toHaveBeenCalledTimes(2);
    force.resolve({ outcome: 'completed', sources: [], skills: [] });
    await forceRequest;

    expect(mocks.checkUpdates).toHaveBeenCalledTimes(3);
    expect(mocks.checkUpdates).toHaveBeenLastCalledWith({
      context,
      mode: 'automatic',
      selection: {
        kind: 'skills',
        skills: [{ context, skillName: 'reviewer' }],
      },
    });
  });

  it('checks only the selected project Context automatically', async () => {
    const projectContext: SkillLocationRef = {
      environment: context.environment,
      scope: { scope: 'project', project_id: 'project-a' },
    };
    useSkillsDataStore.setState({
      snapshots: {
        [contextKey(projectContext)]: {
          skills: [skill({ scope: 'project', skillPath: 'skills/toolkit' })],
          agents: [], pathExists: true, loading: false, error: null, requestId: 1,
        },
      },
    });
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
    setSkills([skill({
      hasUpdate: false,
      updateStatus: 'upToDate',
      skillPath: 'skills/toolkit',
    })]);
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
      outcome: 'completed',
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
      outcome: 'notCompleted',
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
    setSkills([skill({ skillPath: 'skills/toolkit' })]);
    const automatic = deferred<UpdateCheckResponse>();
    const force = deferred<UpdateCheckResponse>();
    mocks.checkUpdates
      .mockImplementationOnce(() => automatic.promise)
      .mockImplementationOnce(() => force.promise);

    const automaticRequest = useSkillsDataStore.getState().syncUpdates(context);
    const forceRequest = useSkillsDataStore.getState().forceCheckUpdates(context, { kind: 'all' });
    expect(useSkillsDataStore.getState().checkingUpdateScopes.has(contextKey(context))).toBe(true);

    automatic.resolve({ outcome: 'completed', sources: [], skills: [] });
    await automaticRequest;
    expect(useSkillsDataStore.getState().checkingUpdateScopes.has(contextKey(context))).toBe(true);

    force.resolve({ outcome: 'completed', sources: [], skills: [] });
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

  it('keeps the last available update when a later check does not complete', async () => {
    setSkills([skill({ hasUpdate: true, updateStatus: 'updateAvailable' })]);
    updateInfoCache.set(contextKey(context), {
      results: [updateInfo('toolkit', { source: 'owner/repo', hasUpdate: true, status: 'updateAvailable' })],
      sources: [sourceInfo('github.com/owner/repo')],
      checkedAt: 100,
      completeness: 'complete',
      outcome: 'completed',
    });
    mocks.checkUpdates.mockResolvedValueOnce({
      outcome: 'notCompleted',
      sources: [sourceInfo('github.com/owner/repo', {
        freshness: 'backingOff',
        lastAttempt: {
          checkedAtEpochMs: 300,
          failure: {
            reason: 'network',
            message: 'must not be shown',
            retryAtEpochMs: 500,
            providerCooldown: false,
          },
        },
      })],
      skills: [updateInfo('toolkit', {
        source: 'owner/repo',
        hasUpdate: false,
        status: 'cannotCheck',
        reason: 'upstreamUnavailable',
        freshness: 'backingOff',
      })],
    } satisfies UpdateCheckResponse);

    const outcome = await useSkillsDataStore.getState().forceCheckUpdates(context, { kind: 'all' });

    expect(outcome).toBe('notCompleted');
    expect(useSkillsDataStore.getState().snapshots[contextKey(context)]).toMatchObject({
      skills: [{
        name: 'toolkit',
        hasUpdate: true,
        updateStatus: 'updateAvailable',
        updateReason: null,
        updateFreshness: 'backingOff',
        updateAttempt: { outcome: 'notCompleted', reason: 'upstreamUnavailable' },
        updateEvidence: {
          source: 'github.com/owner/repo',
          lastAttempt: {
            failure: { reason: 'network', retryAtEpochMs: 500 },
          },
        },
      }],
      updateCheck: { outcome: 'notCompleted' },
    });
  });

  it('keeps a confirmed up-to-date result while a later check is incomplete', async () => {
    setSkills([skill({ hasUpdate: false, updateStatus: 'upToDate' })]);
    mocks.checkUpdates.mockResolvedValueOnce({
      outcome: 'notCompleted',
      sources: [sourceInfo('github.com/owner/repo', {
        freshness: 'backingOff',
        lastAttempt: {
          checkedAtEpochMs: 300,
          failure: {
            reason: 'network',
            message: 'temporary failure',
            retryAtEpochMs: 500,
            providerCooldown: false,
          },
        },
      })],
      skills: [updateInfo('toolkit', {
        source: 'owner/repo',
        hasUpdate: false,
        status: 'cannotCheck',
        reason: 'upstreamUnavailable',
        freshness: 'backingOff',
      })],
    } satisfies UpdateCheckResponse);

    await useSkillsDataStore.getState().forceCheckUpdates(context, { kind: 'all' });

    expect(useSkillsDataStore.getState().snapshots[contextKey(context)]?.skills[0]).toMatchObject({
      hasUpdate: false,
      updateStatus: 'upToDate',
      updateReason: null,
      updateAttempt: { outcome: 'notCompleted' },
    });
    expect(mocks.toastError).not.toHaveBeenCalled();
  });

  it('does not preserve a committed comparison after the source identity changes', async () => {
    setSkills([skill({
      source: 'owner/old-repo',
      gitRef: 'main',
      skillPath: 'skills/toolkit',
      hasUpdate: false,
      updateStatus: 'upToDate',
    })]);
    updateInfoCache.set(contextKey(context), {
      results: [updateInfo('toolkit', {
        source: 'owner/old-repo',
        gitRef: 'main',
        skillPath: 'skills/toolkit',
        hasUpdate: false,
        status: 'upToDate',
      })],
      sources: [],
      checkedAt: 100,
      completeness: 'complete',
      outcome: 'completed',
    });
    mocks.listSkills.mockResolvedValueOnce({
      skills: [skill({
        source: 'owner/new-repo',
        gitRef: 'release',
        skillPath: 'packages/toolkit',
        hasUpdate: false,
        updateStatus: null,
        updateReason: null,
      })],
      agents: [],
      pathExists: true,
    });
    await useSkillsDataStore.getState().refreshContext(context);
    mocks.checkUpdates.mockResolvedValueOnce({
      outcome: 'notCompleted',
      sources: [sourceInfo('github.com/owner/new-repo', {
        requestedRef: 'release',
        resolvedRef: null,
        refRevision: null,
        checkedAtEpochMs: null,
        expiresAtEpochMs: null,
        freshness: 'backingOff',
        lastAttempt: {
          checkedAtEpochMs: 300,
          failure: {
            reason: 'network',
            message: 'temporary failure',
            retryAtEpochMs: 500,
            providerCooldown: false,
          },
        },
      })],
      skills: [updateInfo('toolkit', {
        source: 'owner/new-repo',
        gitRef: 'release',
        skillPath: 'packages/toolkit',
        hasUpdate: false,
        status: 'cannotCheck',
        reason: 'upstreamUnavailable',
        freshness: 'backingOff',
      })],
    } satisfies UpdateCheckResponse);

    await useSkillsDataStore.getState().forceCheckUpdates(context, { kind: 'all' });

    expect(useSkillsDataStore.getState().snapshots[contextKey(context)]?.skills[0]).toMatchObject({
      source: 'owner/new-repo',
      gitRef: 'release',
      skillPath: 'packages/toolkit',
      hasUpdate: false,
      updateStatus: 'cannotCheck',
      updateReason: 'upstreamUnavailable',
      updateAttempt: { outcome: 'notCompleted' },
    });
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
    (useSkillsDataStore.getState() as unknown as { applyUpdateResult: (context: SkillLocationRef, result: UpdateResponse) => void })
      .applyUpdateResult(context, updateResponse('succeeded'));
    expect(useSkillsDataStore.getState().snapshots[contextKey(context)]?.skills[0]).toMatchObject({
      hasUpdate: false, updateStatus: 'upToDate', updateReason: null,
    });
  });

  it('keeps the update display when the workflow reports a failed unit', () => {
    setSkills([skill()]);
    (useSkillsDataStore.getState() as unknown as { applyUpdateResult: (context: SkillLocationRef, result: UpdateResponse) => void })
      .applyUpdateResult(context, updateResponse('failed'));
    expect(useSkillsDataStore.getState().snapshots[contextKey(context)]?.skills[0]?.hasUpdate).toBe(true);
  });

  it('clears a legacy missing-remote-hash result before a failed post-update refresh', async () => {
    setSkills([skill({
      hasUpdate: false,
      canCheckForUpdates: false,
      updateStatus: 'cannotCheck',
      updateReason: 'missing-remote-hash',
    })]);
    mocks.listSkills.mockRejectedValueOnce(new Error('snapshot unavailable'));

    await useSkillsDataStore.getState().applyUpdateResult(context, updateResponse('succeeded'));

    expect(useSkillsDataStore.getState().snapshots[contextKey(context)]?.skills[0]).toMatchObject({
      hasUpdate: false,
      updateStatus: 'upToDate',
      updateReason: null,
    });
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
