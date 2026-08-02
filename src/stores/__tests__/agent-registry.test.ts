import { beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  AgentRuntimeSnapshot,
  AgentSettingsSnapshot,
  AgentDeleteImpact,
  ContextRef,
  CustomAgentDefinition,
  CustomAgentDraftValidation,
  EnvironmentRef,
} from '@/bindings';
import { contextKey } from '@/lib/context';

const api = vi.hoisted(() => ({
  getAgentSettingsSnapshot: vi.fn(),
  listAgents: vi.fn(),
  validateCustomAgentDraft: vi.fn(),
  saveCustomAgent: vi.fn(),
  duplicateCustomAgentDraft: vi.fn(),
  previewCustomAgentDelete: vi.fn(),
  deleteCustomAgent: vi.fn(),
  deleteInvalidCustomAgent: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => api);

import { createAgentRegistryStore } from '../agent-registry';

const host: EnvironmentRef = { kind: 'host' };
const hostGlobal: ContextRef = {
  environment: host,
  scope: { scope: 'global' },
};
const ubuntuProject: ContextRef = {
  environment: { kind: 'wsl', distro_name: 'Ubuntu' },
  scope: { scope: 'project', project_id: 'project-1' },
};

function runtimeSnapshot(context: ContextRef): AgentRuntimeSnapshot {
  return {
    registryRevision: 'registry-1',
    environmentRevision: 'environment-1',
    environment: context.environment,
    availability: 'available',
    projectPath: null,
    agents: {},
  };
}

function settingsSnapshot(revision: string): AgentSettingsSnapshot {
  return {
    registryRevision: revision,
    activeBuiltin: [],
    activeCustom: [],
    disabledConflicts: [],
    invalidCustomRecords: [],
    currentEnvironment: host,
    customStorageIssue: null,
  };
}

function draft(id = 'my-agent'): CustomAgentDefinition {
  return {
    id,
    displayName: id,
    global: { enabled: true, location: 'private', privatePath: null },
    project: { enabled: false, location: 'private', privatePath: null },
    detectionPaths: [],
  };
}

function validation(revision: string): CustomAgentDraftValidation {
  return {
    registryRevision: revision,
    environmentRevision: `environment-${revision}`,
    environment: host,
    resolved: {} as CustomAgentDraftValidation['resolved'],
  };
}

function deleteImpact(revision: string): AgentDeleteImpact {
  return {
    agentId: 'my-agent',
    displayName: 'My Agent',
    registryRevision: revision,
    environmentRevision: `environment-${revision}`,
    scopes: [],
    losesManagementCapability: false,
    filesWillBeDeleted: false,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

describe('agent registry store', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('keeps runtime snapshots isolated by ContextKey', async () => {
    api.listAgents.mockImplementation(async (context: ContextRef) => runtimeSnapshot(context));
    const store = createAgentRegistryStore();

    await Promise.all([store.getState().loadRuntime(hostGlobal), store.getState().loadRuntime(ubuntuProject)]);

    expect(store.getState().runtimeByContext[contextKey(hostGlobal)].data?.environment).toEqual(host);
    expect(store.getState().runtimeByContext[contextKey(ubuntuProject)].data?.environment)
      .toEqual({ kind: 'wsl', distro_name: 'Ubuntu' });
  });

  it('drops an older settings response for the same Environment', async () => {
    const first = deferred<AgentSettingsSnapshot>();
    const second = deferred<AgentSettingsSnapshot>();
    api.getAgentSettingsSnapshot.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const store = createAgentRegistryStore();

    const a = store.getState().loadSettings(hostGlobal);
    const b = store.getState().loadSettings(hostGlobal);
    second.resolve(settingsSnapshot('new'));
    first.resolve(settingsSnapshot('old'));
    await Promise.all([a, b]);

    expect(store.getState().settingsByEnvironment.host.data?.registryRevision).toBe('new');
  });

  it('drops a stale runtime error after a newer request succeeds', async () => {
    const first = deferred<AgentRuntimeSnapshot>();
    const second = deferred<AgentRuntimeSnapshot>();
    api.listAgents.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const store = createAgentRegistryStore();

    const oldLoad = store.getState().loadRuntime(hostGlobal);
    const newLoad = store.getState().loadRuntime(hostGlobal);
    second.resolve(runtimeSnapshot(hostGlobal));
    await newLoad;
    first.reject(new Error('stale failure'));
    await oldLoad;

    expect(store.getState().runtimeByContext[contextKey(hostGlobal)]).toMatchObject({
      state: 'ready',
      error: null,
    });
  });

  it('returns null from a stale validation success while the latest result wins', async () => {
    const oldValidation = deferred<CustomAgentDraftValidation>();
    const latestValidation = deferred<CustomAgentDraftValidation>();
    api.validateCustomAgentDraft
      .mockReturnValueOnce(oldValidation.promise)
      .mockReturnValueOnce(latestValidation.promise);
    const store = createAgentRegistryStore();

    const oldRequest = store.getState().validateDraft(hostGlobal, draft('old-agent'));
    const latestRequest = store.getState().validateDraft(hostGlobal, draft('latest-agent'));
    latestValidation.resolve(validation('latest'));
    await expect(latestRequest).resolves.toEqual(validation('latest'));
    oldValidation.resolve(validation('old'));
    await expect(oldRequest).resolves.toBeNull();

    expect(store.getState()).not.toHaveProperty('validationByContext');
  });

  it('returns null from a stale validation error without rejecting', async () => {
    const oldValidation = deferred<CustomAgentDraftValidation>();
    const latestValidation = deferred<CustomAgentDraftValidation>();
    const staleError = { kind: 'environmentUnavailable', data: { environment: host, message: 'old' } };
    api.validateCustomAgentDraft
      .mockReturnValueOnce(oldValidation.promise)
      .mockReturnValueOnce(latestValidation.promise);
    const store = createAgentRegistryStore();

    const oldRequest = store.getState().validateDraft(hostGlobal, draft('old-agent'));
    const latestRequest = store.getState().validateDraft(hostGlobal, draft('latest-agent'));
    latestValidation.resolve(validation('latest'));
    await expect(latestRequest).resolves.toEqual(validation('latest'));
    oldValidation.reject(staleError);
    await expect(oldRequest).resolves.toBeNull();

    expect(store.getState()).not.toHaveProperty('validationByContext');
  });

  it('keeps submit validation independent from an overlapping background validation', async () => {
    const backgroundValidation = deferred<CustomAgentDraftValidation>();
    const submitValidation = deferred<CustomAgentDraftValidation>();
    api.validateCustomAgentDraft
      .mockReturnValueOnce(backgroundValidation.promise)
      .mockReturnValueOnce(submitValidation.promise);
    const store = createAgentRegistryStore();

    const backgroundRequest = store.getState().validateDraft(
      hostGlobal,
      draft('background-agent'),
      'background',
    );
    const submitRequest = store.getState().validateDraft(
      hostGlobal,
      draft('submitted-agent'),
      'submit',
    );
    submitValidation.resolve(validation('submit'));
    await expect(submitRequest).resolves.toEqual(validation('submit'));
    backgroundValidation.resolve(validation('background'));
    await expect(backgroundRequest).resolves.toEqual(validation('background'));
  });

  it('returns delete impact without retaining transient preview state', async () => {
    api.previewCustomAgentDelete.mockResolvedValue(deleteImpact('registry-1'));
    const store = createAgentRegistryStore();

    await store.getState().loadDeleteImpact(hostGlobal, 'my-agent', 'registry-1');
    await store.getState().loadDeleteImpact(hostGlobal, 'my-agent', 'registry-2');

    expect(store.getState()).not.toHaveProperty('deleteImpactByKey');
    expect(api.previewCustomAgentDelete).toHaveBeenCalledTimes(2);
  });

  it('drops an in-flight delete impact from an older registry revision', async () => {
    const oldImpact = deferred<AgentDeleteImpact>();
    const newImpact = deferred<AgentDeleteImpact>();
    api.previewCustomAgentDelete
      .mockReturnValueOnce(oldImpact.promise)
      .mockReturnValueOnce(newImpact.promise);
    const store = createAgentRegistryStore();

    const oldRequest = store.getState().loadDeleteImpact(hostGlobal, 'my-agent', 'registry-1');
    const newRequest = store.getState().loadDeleteImpact(hostGlobal, 'my-agent', 'registry-2');
    newImpact.resolve(deleteImpact('registry-2'));
    await expect(newRequest).resolves.toEqual(deleteImpact('registry-2'));
    oldImpact.resolve(deleteImpact('registry-1'));
    await expect(oldRequest).resolves.toBeNull();

    expect(store.getState()).not.toHaveProperty('deleteImpactByKey');
  });

  it('returns null from a stale delete-impact error while the latest error stays authoritative', async () => {
    const oldImpact = deferred<AgentDeleteImpact>();
    const latestImpact = deferred<AgentDeleteImpact>();
    const latestError = { kind: 'environmentUnavailable', data: { environment: host, message: 'latest' } };
    const staleError = { kind: 'environmentUnavailable', data: { environment: host, message: 'old' } };
    api.previewCustomAgentDelete
      .mockReturnValueOnce(oldImpact.promise)
      .mockReturnValueOnce(latestImpact.promise);
    const store = createAgentRegistryStore();

    const oldRequest = store.getState().loadDeleteImpact(hostGlobal, 'my-agent', 'registry-1');
    const latestRequest = store.getState().loadDeleteImpact(hostGlobal, 'my-agent', 'registry-2');
    latestImpact.reject(latestError);
    await expect(latestRequest).rejects.toEqual(latestError);
    oldImpact.reject(staleError);
    await expect(oldRequest).resolves.toBeNull();

    expect(store.getState()).not.toHaveProperty('deleteImpactByKey');
  });

});
