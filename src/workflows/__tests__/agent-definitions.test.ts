import { beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  AgentRuntimeSnapshot,
  AgentSettingsSnapshot,
  SkillLocationRef,
  CustomAgentDefinition,
  EnvironmentRef,
} from '@/bindings';
import { contextKey } from '@/lib/context';

const api = vi.hoisted(() => ({
  getAgentSettingsSnapshot: vi.fn(),
  listAgents: vi.fn(),
  validateCustomAgentDraft: vi.fn(),
  saveCustomAgent: vi.fn(),
  previewCustomAgentDelete: vi.fn(),
  deleteCustomAgent: vi.fn(),
  deleteInvalidCustomAgent: vi.fn(),
  getDefaultTargetAgents: vi.fn(),
  saveDefaultTargetAgents: vi.fn(),
  listSkills: vi.fn(),
  checkUpdates: vi.fn(),
  previewUpdate: vi.fn(),
  updateSkill: vi.fn(),
  updateSkillsBatch: vi.fn(),
  checkSkillAudit: vi.fn(),
  getInstallWizardSession: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => api);

import { agentDefinitionWorkflow } from '../agent-definitions';
import { useAgentRegistryStore } from '@/stores/agent-registry';
import { useSkillsDataStore } from '@/stores/skills-data';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';
import { useMutationStore } from '@/stores/mutation';
import { BusinessWriteBlockedError } from '@/hooks/useBusinessWriteBlocked';

const native: EnvironmentRef = { kind: 'native' };
const ubuntu: EnvironmentRef = { kind: 'wsl', distro_name: 'Ubuntu' };
const nativeGlobal: SkillLocationRef = { environment: native, scope: { scope: 'global' } };
const ubuntuGlobal: SkillLocationRef = { environment: ubuntu, scope: { scope: 'global' } };

function settings(environment: EnvironmentRef, revision: string): AgentSettingsSnapshot {
  return {
    registryRevision: revision,
    activeBuiltin: [],
    activeCustom: [],
    disabledConflicts: [],
    invalidCustomRecords: [],
    currentEnvironment: environment,
    customStorageIssue: null,
  };
}

function runtime(context: SkillLocationRef, revision: string): AgentRuntimeSnapshot {
  return {
    registryRevision: revision,
    environmentRevision: `environment-${revision}`,
    environment: context.environment,
    availability: 'available',
    projectPath: null,
    agents: {},
  };
}

function draft(): CustomAgentDefinition {
  return {
    id: 'custom-agent',
    displayName: 'Custom Agent',
    global: { enabled: true, location: 'private', privatePath: null },
    project: { enabled: false, location: 'private', privatePath: null },
    detectionPaths: [],
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

describe('Agent definition workflow ownership', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useAgentRegistryStore.setState({
      settingsByEnvironment: {
        native: { data: settings(native, 'registry-1'), state: 'ready', requestId: 1, error: null },
        'wsl:ubuntu': { data: settings(ubuntu, 'registry-1'), state: 'ready', requestId: 1, error: null },
      },
      runtimeByContext: {
        [contextKey(nativeGlobal)]: { data: runtime(nativeGlobal, 'registry-1'), state: 'ready', requestId: 1, error: null },
        [contextKey(ubuntuGlobal)]: { data: runtime(ubuntuGlobal, 'registry-1'), state: 'ready', requestId: 1, error: null },
      },
    });
    useSkillsDataStore.setState({
      snapshots: {
        [contextKey(nativeGlobal)]: {} as never,
        [contextKey(ubuntuGlobal)]: {} as never,
      },
    });
    useMutationStore.setState({ activeMutation: null });
    useInstallWizardSessionStore.setState({
      revision: 0, active: false, loading: false, hasConfirmedSnapshot: false,
      syncError: null, monitorRetryRevision: 0, snapshotVersion: 0,
    });
    api.getInstallWizardSession.mockResolvedValue({ revision: 1, active: true });
  });

  it('invalidates every Agent projection, rejects old in-flight responses, then accepts the returned Settings snapshot', async () => {
    const oldRuntime = deferred<AgentRuntimeSnapshot>();
    api.listAgents.mockReturnValue(oldRuntime.promise);
    api.saveCustomAgent.mockResolvedValue(settings(ubuntu, 'registry-2'));

    const staleLoad = useAgentRegistryStore.getState().loadRuntime(nativeGlobal);
    const result = await agentDefinitionWorkflow.save(nativeGlobal, draft(), null, 'registry-1');
    oldRuntime.resolve(runtime(nativeGlobal, 'registry-1'));
    await staleLoad;

    expect(result?.registryRevision).toBe('registry-2');
    expect(useAgentRegistryStore.getState().runtimeByContext).toEqual({});
    expect(useAgentRegistryStore.getState()).not.toHaveProperty('validationByContext');
    expect(useAgentRegistryStore.getState()).not.toHaveProperty('deleteImpactByKey');
    expect(useAgentRegistryStore.getState().settingsByEnvironment).toEqual({
      'wsl:ubuntu': expect.objectContaining({
        state: 'ready',
        data: expect.objectContaining({ registryRevision: 'registry-2' }),
      }),
    });
    expect(useSkillsDataStore.getState().snapshots).toEqual({});
  });

  it('does not send Agent definition writes while the install wizard is active', async () => {
    useInstallWizardSessionStore.setState({ revision: 1, active: true });

    await expect(agentDefinitionWorkflow.save(nativeGlobal, draft(), null, 'registry-1'))
      .rejects.toEqual(new BusinessWriteBlockedError('installWizardActive'));
    expect(api.saveCustomAgent).not.toHaveBeenCalled();
  });

  it('returns null without invalidating projections when installation wins admission', async () => {
    api.saveCustomAgent.mockRejectedValue({
      kind: 'application',
      error: { kind: 'installWizardActive' },
    });

    await expect(agentDefinitionWorkflow.save(nativeGlobal, draft(), null, 'registry-1'))
      .resolves.toBeNull();

    expect(useAgentRegistryStore.getState().runtimeByContext).not.toEqual({});
    expect(useSkillsDataStore.getState().snapshots).not.toEqual({});
  });

  it('returns null when installation wins Agent deletion admission', async () => {
    api.deleteCustomAgent.mockRejectedValue({
      kind: 'application',
      error: { kind: 'installWizardActive' },
    });

    await expect(agentDefinitionWorkflow.delete(nativeGlobal, 'custom-agent', 'registry-1'))
      .resolves.toBeNull();

    expect(useAgentRegistryStore.getState().runtimeByContext).not.toEqual({});
  });
});
