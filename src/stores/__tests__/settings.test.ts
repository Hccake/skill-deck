import { beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  ActiveMutation,
  AgentInfo,
  ContextRef,
  DefaultTargetAgents,
  EnvironmentRef,
} from '@/bindings';
import { makeAgentScopeTarget } from '@/test-utils';

const mockGetDefaultTargetAgents = vi.fn();
const mockSaveDefaultTargetAgents = vi.fn();
const mockListAgents = vi.fn();

vi.mock('@/hooks/useTauriApi', () => ({
  getDefaultTargetAgents: (...args: unknown[]) => mockGetDefaultTargetAgents(...args),
  saveDefaultTargetAgents: (...args: unknown[]) => mockSaveDefaultTargetAgents(...args),
  listAgents: (...args: unknown[]) => mockListAgents(...args),
}));

import { useSettingsStore } from '../settings';
import { useMutationStore } from '../mutation';

const host: EnvironmentRef = { kind: 'host' };
const ubuntu: EnvironmentRef = { kind: 'wsl', distro_name: 'Ubuntu' };
const debian: EnvironmentRef = { kind: 'wsl', distro_name: 'Debian' };

const activeMutation: ActiveMutation = {
  kind: 'install',
  context: { environment: host, scope: { scope: 'global' } },
  id: 'mutation-1',
  phase: 'preparing',
  progress: null,
  cancelable: false,
};

const agents: AgentInfo[] = [
  {
    id: 'antigravity',
    name: 'Antigravity',
    skillsDir: '.agents/skills',
    globalSkillsDir: '~/.gemini/antigravity/skills',
    detected: true,
    targets: {
      global: makeAgentScopeTarget({ automatic: false, path: '~/.gemini/antigravity/skills' }),
      project: makeAgentScopeTarget({ automatic: true, path: '.agents/skills' }),
    },
  },
  {
    id: 'claude-code',
    name: 'Claude Code',
    skillsDir: '.claude/skills',
    globalSkillsDir: '~/.claude/skills',
    detected: true,
    targets: {
      global: makeAgentScopeTarget({ automatic: false, path: '~/.claude/skills' }),
      project: makeAgentScopeTarget({ automatic: false, path: '.claude/skills' }),
    },
  },
];

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
    mockListAgents.mockResolvedValue(agents);
    mockGetDefaultTargetAgents.mockResolvedValue(null);
    mockSaveDefaultTargetAgents.mockResolvedValue(undefined);
    useSettingsStore.setState({ agentDefaultsByEnvironment: {} });
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
    expect(useSettingsStore.getState().agentDefaultsByEnvironment['wsl:Ubuntu'].defaults)
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

    const snapshot = useSettingsStore.getState().agentDefaultsByEnvironment['wsl:Ubuntu'];
    expect(snapshot.defaults).toEqual({ global: ['claude-code'], project: [] });
    expect(snapshot.loadRequestId).toBe(2);
  });

  it('stores a typed load error only for the failing environment', async () => {
    mockListAgents.mockImplementation((context: ContextRef) =>
      context.environment.kind === 'host'
        ? Promise.reject(new Error('host unavailable'))
        : Promise.resolve(agents));

    await Promise.all([
      useSettingsStore.getState().loadAgentDefaults(host),
      useSettingsStore.getState().loadAgentDefaults(ubuntu),
    ]);

    expect(useSettingsStore.getState().agentDefaultsByEnvironment.host.loadState).toBe('error');
    expect(useSettingsStore.getState().agentDefaultsByEnvironment.host.error?.kind).toBe('custom');
    expect(useSettingsStore.getState().agentDefaultsByEnvironment['wsl:Ubuntu'].loadState)
      .toBe('ready');
  });

  it('sets saving immediately and keeps the captured environment while another loads', async () => {
    const save = deferred<void>();
    mockSaveDefaultTargetAgents.mockReturnValue(save.promise);
    useSettingsStore.setState({
      agentDefaultsByEnvironment: {
        'wsl:Ubuntu': readySnapshot({ global: [], project: [] }),
      },
    });
    const defaults = { global: ['claude-code'], project: [] };

    const pendingSave = useSettingsStore.getState().saveAgentDefaults(ubuntu, defaults);
    void useSettingsStore.getState().loadAgentDefaults(debian);

    expect(useSettingsStore.getState().agentDefaultsByEnvironment['wsl:Ubuntu'].saving).toBe(true);
    expect(mockSaveDefaultTargetAgents).toHaveBeenCalledWith(
      { environment: ubuntu, scope: { scope: 'global' } },
      defaults,
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
        'wsl:Ubuntu': readySnapshot(ubuntuDefaults),
        'wsl:Debian': readySnapshot(debianDefaults),
      },
    });

    await useSettingsStore.getState().saveAgentDefaults(
      ubuntu,
      { global: ['claude-code'], project: [] },
    );

    expect(useSettingsStore.getState().agentDefaultsByEnvironment['wsl:Ubuntu'].defaults)
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
        'wsl:Ubuntu': readySnapshot({ global: [], project: [] }),
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

    const snapshot = useSettingsStore.getState().agentDefaultsByEnvironment['wsl:Ubuntu'];
    expect(snapshot.defaults).toEqual(newest);
    expect(snapshot.saving).toBe(false);
    expect(snapshot.saveRequestId).toBe(2);
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
});
