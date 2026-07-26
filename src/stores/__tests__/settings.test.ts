import { beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  ActiveMutation,
  ContextRef,
  DefaultTargetAgents,
  EnvironmentRef,
  GithubCredentialStatus,
  ResolvedAgent,
} from '@/bindings';
import { makeAgentRuntimeSnapshot, makeResolvedAgent } from '@/test-utils';

const mockGetDefaultTargetAgents = vi.fn();
const mockSaveDefaultTargetAgents = vi.fn();
const mockListAgents = vi.fn();
const mockListAgentSelectionGroups = vi.fn();
const mockGetGithubCredentialStatus = vi.fn();
const mockSaveGithubCredential = vi.fn();
const mockClearGithubCredential = vi.fn();

vi.mock('@/hooks/useTauriApi', () => ({
  getDefaultTargetAgents: (...args: unknown[]) => mockGetDefaultTargetAgents(...args),
  saveDefaultTargetAgents: (...args: unknown[]) => mockSaveDefaultTargetAgents(...args),
  listAgents: (...args: unknown[]) => mockListAgents(...args),
  listAgentSelectionGroups: (...args: unknown[]) => mockListAgentSelectionGroups(...args),
  getGithubCredentialStatus: (...args: unknown[]) => mockGetGithubCredentialStatus(...args),
  saveGithubCredential: (...args: unknown[]) => mockSaveGithubCredential(...args),
  clearGithubCredential: (...args: unknown[]) => mockClearGithubCredential(...args),
  getAgentSettingsSnapshot: vi.fn(),
  validateCustomAgentDraft: vi.fn(),
  saveCustomAgent: vi.fn(),
  duplicateCustomAgentDraft: vi.fn(),
  previewCustomAgentDelete: vi.fn(),
  deleteCustomAgent: vi.fn(),
  deleteInvalidCustomAgent: vi.fn(),
}));

import { useSettingsStore } from '../settings';
import { useAgentRegistryStore } from '../agent-registry';
import { useMutationStore } from '../mutation';
import { contextKey } from '@/lib/context';

const host: EnvironmentRef = { kind: 'host' };
const ubuntu: EnvironmentRef = { kind: 'wsl', distro_name: 'Ubuntu' };
const debian: EnvironmentRef = { kind: 'wsl', distro_name: 'Debian' };

const verifiedCredential: GithubCredentialStatus = {
  source: 'keyring',
  storage: 'available',
  validation: 'verified',
  account: 'octocat',
  rateLimitRemaining: 4_999,
  rateLimitLimit: 5_000,
  rateLimitResetAtEpochMs: 2_000,
  retryAtEpochMs: null,
};

const activeMutation: ActiveMutation = {
  kind: 'install',
  context: { environment: host, scope: { scope: 'global' } },
  id: 'mutation-1',
  phase: 'preparing',
  progress: null,
  cancelable: false,
};

const agents: ResolvedAgent[] = [
  makeResolvedAgent({
    id: 'antigravity',
    displayName: 'Antigravity',
    global: {
      readsShared: false,
      privatePath: '~/.gemini/antigravity/skills',
    },
    project: { readsShared: true, sharedPath: './.agents/skills' },
  }),
  makeResolvedAgent({
    id: 'claude-code',
    displayName: 'Claude Code',
    global: { readsShared: false, privatePath: '~/.claude/skills' },
    project: {
      readsShared: false,
      privatePath: '.claude/skills',
    },
  }),
];
const agentRuntimeSnapshot = makeAgentRuntimeSnapshot(agents);

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function readySnapshot(defaults: DefaultTargetAgents) {
  return {
    agents,
    selectionGroups: { global: [], project: [] },
    registryRevision: agentRuntimeSnapshot.registryRevision,
    defaults,
    loadState: 'ready' as const,
    loadRequestId: 1,
    saveRequestId: 0,
    saving: false,
    error: null,
  };
}

describe('useSettingsStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockListAgents.mockResolvedValue(agentRuntimeSnapshot);
    mockListAgentSelectionGroups.mockResolvedValue({ global: [], project: [] });
    mockGetDefaultTargetAgents.mockResolvedValue(null);
    mockSaveDefaultTargetAgents.mockResolvedValue(undefined);
    mockGetGithubCredentialStatus.mockResolvedValue(verifiedCredential);
    mockClearGithubCredential.mockResolvedValue({
      cleared: true,
      status: { ...verifiedCredential, source: 'none', validation: 'unconfigured' },
    });
    useSettingsStore.setState({
      agentDefaultsByEnvironment: {},
      githubCredential: {
        status: null,
        loadState: 'idle',
        requestId: 0,
        saving: false,
        clearing: false,
        error: null,
      },
    });
    useAgentRegistryStore.setState({ settingsByEnvironment: {}, runtimeByContext: {} });
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
  });

  it('updates theme and locale independently of environment snapshots', () => {
    useSettingsStore.setState({ theme: 'light', locale: 'en' });
    useSettingsStore.getState().toggleTheme();
    useSettingsStore.getState().setLocale('zh-CN');

    expect(useSettingsStore.getState().theme).toBe('dark');
    expect(useSettingsStore.getState().locale).toBe('zh-CN');
    expect(useSettingsStore.getState().agentDefaultsByEnvironment).toEqual({});
  });

  it('loads filtered persisted defaults without a hidden migration save', async () => {
    mockGetDefaultTargetAgents.mockResolvedValue({
      global: ['antigravity', 'claude-code'],
      project: ['antigravity', 'claude-code'],
    });

    await useSettingsStore.getState().loadAgentDefaults(host);

    expect(useSettingsStore.getState().agentDefaultsByEnvironment.host.defaults).toEqual({
      global: ['antigravity', 'claude-code'],
      project: ['claude-code'],
    });
    expect(mockSaveDefaultTargetAgents).not.toHaveBeenCalled();
  });

  it('uses the registry-owned runtime snapshot when loading defaults', async () => {
    await useSettingsStore.getState().loadAgentDefaults(host);

    expect(useAgentRegistryStore.getState().runtimeByContext[contextKey({
      environment: host,
      scope: { scope: 'global' },
    })]?.data).toEqual(agentRuntimeSnapshot);
  });

  it('falls back to compatible CLI defaults when scoped defaults are absent', async () => {
    await useSettingsStore.getState().loadAgentDefaults(host);

    expect(useSettingsStore.getState().agentDefaultsByEnvironment.host.defaults).toEqual({
      global: ['claude-code'],
      project: ['claude-code'],
    });
  });

  it('keeps Host and Ubuntu loads isolated when they resolve out of order', async () => {
    const hostDefaults = deferred<DefaultTargetAgents | null>();
    const ubuntuDefaults = deferred<DefaultTargetAgents | null>();
    mockGetDefaultTargetAgents.mockImplementation((context: ContextRef) =>
      context.environment.kind === 'host' ? hostDefaults.promise : ubuntuDefaults.promise);

    const hostLoad = useSettingsStore.getState().loadAgentDefaults(host);
    const ubuntuLoad = useSettingsStore.getState().loadAgentDefaults(ubuntu);
    ubuntuDefaults.resolve({ global: ['claude-code'], project: [] });
    await ubuntuLoad;
    hostDefaults.resolve({ global: [], project: ['claude-code'] });
    await hostLoad;

    expect(useSettingsStore.getState().agentDefaultsByEnvironment.host.defaults)
      .toEqual({ global: [], project: ['claude-code'] });
    expect(useSettingsStore.getState().agentDefaultsByEnvironment['wsl:ubuntu'].defaults)
      .toEqual({ global: ['claude-code'], project: [] });
  });

  it('ignores an older load result for the same environment', async () => {
    const first = deferred<DefaultTargetAgents | null>();
    const second = deferred<DefaultTargetAgents | null>();
    mockGetDefaultTargetAgents
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    const firstLoad = useSettingsStore.getState().loadAgentDefaults(ubuntu);
    const secondLoad = useSettingsStore.getState().loadAgentDefaults(ubuntu);
    second.resolve({ global: ['claude-code'], project: [] });
    await secondLoad;
    first.resolve({ global: [], project: ['claude-code'] });
    await firstLoad;

    const snapshot = useSettingsStore.getState().agentDefaultsByEnvironment['wsl:ubuntu'];
    expect(snapshot.defaults).toEqual({ global: ['claude-code'], project: [] });
    expect(snapshot.loadRequestId).toBe(2);
  });

  it('stores a typed load error only for the failing environment', async () => {
    mockListAgents.mockImplementation((context: ContextRef) =>
      context.environment.kind === 'host'
        ? Promise.reject(new Error('host unavailable'))
        : Promise.resolve(agentRuntimeSnapshot));

    await Promise.all([
      useSettingsStore.getState().loadAgentDefaults(host),
      useSettingsStore.getState().loadAgentDefaults(ubuntu),
    ]);

    expect(useSettingsStore.getState().agentDefaultsByEnvironment.host.loadState).toBe('error');
    expect(useSettingsStore.getState().agentDefaultsByEnvironment.host.error?.kind).toBe('custom');
    expect(useSettingsStore.getState().agentDefaultsByEnvironment['wsl:ubuntu'].loadState)
      .toBe('ready');
  });

  it('preserves the structured runtime error from the registry snapshot', async () => {
    const error = {
      kind: 'environmentUnavailable',
      data: { environment: host, message: 'Host inspection is unavailable' },
    } as const;
    mockListAgents.mockRejectedValue(error);

    await useSettingsStore.getState().loadAgentDefaults(host);

    expect(useSettingsStore.getState().agentDefaultsByEnvironment.host.error).toEqual(error);
  });

  it('sets saving immediately and keeps the captured environment while another loads', async () => {
    const save = deferred<void>();
    mockSaveDefaultTargetAgents.mockReturnValue(save.promise);
    useSettingsStore.setState({
      agentDefaultsByEnvironment: {
        'wsl:ubuntu': readySnapshot({ global: [], project: [] }),
      },
    });
    const defaults = { global: ['claude-code'], project: [] };

    const pendingSave = useSettingsStore.getState().saveAgentDefaults(ubuntu, defaults);
    void useSettingsStore.getState().loadAgentDefaults(debian);

    expect(useSettingsStore.getState().agentDefaultsByEnvironment['wsl:ubuntu'].saving).toBe(true);
    expect(mockSaveDefaultTargetAgents).toHaveBeenCalledWith(
      { environment: ubuntu, scope: { scope: 'global' } },
      defaults,
      'registry-1',
    );
    save.resolve();
    await pendingSave;
  });

  it('rolls back only the failed environment', async () => {
    mockSaveDefaultTargetAgents.mockRejectedValue(new Error('save failed'));
    const ubuntuDefaults = { global: [], project: ['claude-code'] };
    const debianDefaults = { global: ['claude-code'], project: [] };
    useSettingsStore.setState({
      agentDefaultsByEnvironment: {
        'wsl:ubuntu': readySnapshot(ubuntuDefaults),
        'wsl:Debian': readySnapshot(debianDefaults),
      },
    });

    await useSettingsStore.getState().saveAgentDefaults(
      ubuntu,
      { global: ['claude-code'], project: [] },
    );

    expect(useSettingsStore.getState().agentDefaultsByEnvironment['wsl:ubuntu'].defaults)
      .toEqual(ubuntuDefaults);
    expect(useSettingsStore.getState().agentDefaultsByEnvironment['wsl:Debian'].defaults)
      .toEqual(debianDefaults);
  });

  it('does not let an older failed save roll back a newer success', async () => {
    const first = deferred<void>();
    const second = deferred<void>();
    mockSaveDefaultTargetAgents
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    useSettingsStore.setState({
      agentDefaultsByEnvironment: {
        'wsl:ubuntu': readySnapshot({ global: [], project: [] }),
      },
    });
    const oldSave = useSettingsStore.getState().saveAgentDefaults(
      ubuntu,
      { global: ['claude-code'], project: [] },
    );
    const newest = { global: [], project: ['claude-code'] };
    const newSave = useSettingsStore.getState().saveAgentDefaults(ubuntu, newest);

    second.resolve();
    await newSave;
    first.reject(new Error('stale failure'));
    await oldSave;

    const snapshot = useSettingsStore.getState().agentDefaultsByEnvironment['wsl:ubuntu'];
    expect(snapshot.defaults).toEqual(newest);
    expect(snapshot.saving).toBe(false);
    expect(snapshot.saveRequestId).toBe(2);
  });

  it('marks a failed optimistic default save as stale until it is refreshed', async () => {
    mockSaveDefaultTargetAgents.mockRejectedValue(new Error('save failed'));
    useSettingsStore.setState({
      agentDefaultsByEnvironment: {
        host: readySnapshot({ global: [], project: [] }),
      },
    });

    await useSettingsStore.getState().saveAgentDefaults(
      host,
      { global: ['claude-code'], project: [] },
    );

    expect(useSettingsStore.getState().agentDefaultsByEnvironment.host.loadState).toBe('stale');
  });

  it('does not optimistically update or call the API while a mutation is active', async () => {
    const original = { global: [], project: ['claude-code'] };
    useSettingsStore.setState({
      agentDefaultsByEnvironment: { host: readySnapshot(original) },
    });
    useMutationStore.setState({ activeMutation });

    await useSettingsStore.getState().saveAgentDefaults(
      host,
      { global: ['claude-code'], project: [] },
    );

    expect(useSettingsStore.getState().agentDefaultsByEnvironment.host.defaults).toEqual(original);
    expect(mockSaveDefaultTargetAgents).not.toHaveBeenCalled();
  });

  it('loads GitHub credential status without storing the token', async () => {
    await useSettingsStore.getState().loadGithubCredential();

    expect(useSettingsStore.getState().githubCredential.status).toEqual(verifiedCredential);
    expect(JSON.stringify(useSettingsStore.getState())).not.toContain('secret-token');
  });

  it('does not replace the active credential when a new token is invalid', async () => {
    mockSaveGithubCredential.mockResolvedValue({
      saved: false,
      status: {
        ...verifiedCredential,
        source: 'none',
        validation: 'invalid',
        account: null,
      },
    });
    useSettingsStore.setState((state) => ({
      githubCredential: {
        ...state.githubCredential,
        status: verifiedCredential,
        loadState: 'ready',
      },
    }));

    const result = await useSettingsStore.getState().saveGithubCredential('secret-token');

    expect(result?.saved).toBe(false);
    expect(useSettingsStore.getState().githubCredential.status).toEqual(verifiedCredential);
    expect(JSON.stringify(useSettingsStore.getState())).not.toContain('secret-token');
  });
});
