import { beforeEach, describe, expect, it, vi } from 'vitest';
import { repairSkillSource } from '../skill-repair';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';
import { useMutationStore } from '@/stores/mutation';

const mocks = vi.hoisted(() => ({ getInstallWizardSession: vi.fn(), getInstallAgentSelection: vi.fn() }));

vi.mock('@/hooks/useTauriApi', () => ({
  fetchAvailable: vi.fn(),
  installSkills: vi.fn(),
  acquireSelectedPayloads: vi.fn(),
  previewInstall: vi.fn(),
  getInstallAgentSelection: (...args: unknown[]) => mocks.getInstallAgentSelection(...args),
  getInstallWizardSession: () => mocks.getInstallWizardSession(),
}));

const context = {
  environment: { kind: 'host' },
  scope: { scope: 'global' },
} as const;

function request(stopRequested = () => false) {
  return {
    context,
    source: 'owner/repo',
    skillName: 'toolkit',
    agents: ['claude-code'],
    privateAdaptedAgents: ['claude-code'],
    privateCopyAgents: [],
    acknowledgeRisk: true,
    operationId: 'repair-1',
    stopRequested,
  };
}

function api() {
  return {
    fetchAvailable: vi.fn().mockResolvedValue({
      discoverySession: { sessionId: 'discovery-1' },
      riskPolicy: { kind: 'none', code: null },
      skills: [{ name: 'toolkit', relativePath: 'skills/toolkit' }],
    }),
    prepareInstall: vi.fn().mockResolvedValue({
      status: 'ready',
      prepared: {
        request: { context },
        preview: { token: { generation: 'preview-1' } },
      },
    }),
    installSkills: vi.fn().mockResolvedValue({
      units: [{ unitId: 'toolkit', status: 'succeeded' }],
    }),
    getInstallAgentSelection: vi.fn().mockResolvedValue({
      selection: {
        agents: [], installOptions: [], groups: [], initialSelectedOptionIds: [],
        unavailableExplicitAgents: [], userModeOptionIds: [], revision: 'selection-1',
      },
      defaultSelectionWarning: null,
    }),
  };
}

describe('repairSkillSource', () => {
  beforeEach(() => {
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
    useInstallWizardSessionStore.setState({
      revision: 0, active: false, loading: false, hasConfirmedSnapshot: false,
      syncError: null, monitorRetryRevision: 0, snapshotVersion: 0,
    });
    mocks.getInstallWizardSession.mockResolvedValue({ revision: 1, active: true });
  });

  it('does not begin source repair while the install wizard is active', async () => {
    const workflowApi = api();
    useInstallWizardSessionStore.setState({ revision: 1, active: true });

    await expect(repairSkillSource(request(), workflowApi as never)).resolves.toEqual({
      status: 'blocked',
    });
    expect(workflowApi.fetchAvailable).not.toHaveBeenCalled();
    expect(workflowApi.prepareInstall).not.toHaveBeenCalled();
    expect(workflowApi.installSkills).not.toHaveBeenCalled();
  });

  it('returns a typed success after discovery, preparation, and execution', async () => {
    const workflowApi = api();

    await expect(repairSkillSource(request(), workflowApi as never)).resolves.toEqual({
      status: 'succeeded',
      response: { units: [{ unitId: 'toolkit', status: 'succeeded' }] },
    });
    expect(workflowApi.fetchAvailable).toHaveBeenCalledWith(context, 'owner/repo', 'repair-1');
  });

  it('preserves an existing extra installation for an Agent that also reads the shared directory', async () => {
    const workflowApi = api();
    workflowApi.getInstallAgentSelection.mockResolvedValue({
      selection: {
        agents: [{
          kind: 'standard',
          id: 'cursor',
          displayName: 'Cursor',
          detection: 'detected',
          directoryAccess: 'both',
          installOptionId: 'cursor-own-directory',
          groupId: null,
        }],
        installOptions: [{
          id: 'cursor-own-directory',
          kind: 'standardDirectory',
          agentIds: ['cursor'],
          displayName: 'Cursor',
          path: '~/.cursor/skills',
          groupId: null,
          selectable: true,
          modeConstraint: 'userSelectable',
          disabledReason: null,
        }],
        groups: [],
        initialSelectedOptionIds: [],
        unavailableExplicitAgents: [],
        userModeOptionIds: ['cursor-own-directory'],
        revision: 'selection-1',
      },
      defaultSelectionWarning: null,
    });

    await repairSkillSource({
      ...request(),
      agents: ['cursor'],
      privateAdaptedAgents: [],
      privateCopyAgents: ['cursor'],
    }, workflowApi as never);

    expect(workflowApi.prepareInstall).toHaveBeenCalledWith(expect.objectContaining({
      agentSelection: {
        revision: 'selection-1',
        selectedOptionIds: ['cursor-own-directory'],
        requestedMode: 'copy',
      },
    }));
  });

  it('returns a failed outcome when the single Skill mutation does not succeed', async () => {
    const workflowApi = api();
    workflowApi.installSkills.mockResolvedValue({
      units: [{ unitId: 'toolkit', status: 'failed' }],
    });

    await expect(repairSkillSource(request(), workflowApi as never)).resolves.toEqual({
      status: 'failed',
      stage: 'execution',
      error: null,
    });
  });

  it('returns blocked when installation wins repair execution admission', async () => {
    const workflowApi = api();
    workflowApi.installSkills.mockRejectedValue({ kind: 'installWizardActive' });

    await expect(repairSkillSource(request(), workflowApi as never))
      .resolves.toEqual({ status: 'blocked' });
  });

  it('preserves a recovery action from the single Skill mutation', async () => {
    const workflowApi = api();
    const recovery = { resourceId: 'recovery-1', suggestedActionCode: 'reviewChanges' } as const;
    workflowApi.installSkills.mockResolvedValue({
      units: [{ unitId: 'toolkit', status: 'recoveryRequired', recovery }],
    });

    await expect(repairSkillSource(request(), workflowApi as never)).resolves.toEqual({
      status: 'recoveryRequired',
      response: { units: [{ unitId: 'toolkit', status: 'recoveryRequired', recovery }] },
      recovery: [recovery],
    });
  });

  it('stops before preparation when cancellation is requested after discovery', async () => {
    const workflowApi = api();
    const stopRequested = vi.fn()
      .mockReturnValueOnce(false)
      .mockReturnValue(true);

    await expect(repairSkillSource(request(stopRequested), workflowApi as never))
      .resolves.toEqual({ status: 'stopped' });
    expect(workflowApi.prepareInstall).not.toHaveBeenCalled();
  });
});
