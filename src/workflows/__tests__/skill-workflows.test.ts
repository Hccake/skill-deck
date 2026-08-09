import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentSelectionSubmission, SkillLocationRef, InstalledSkill, ManageAgentSelectionSnapshot, ManageAgentsPreview, RemovePreview } from '@/bindings';
import { makeAgentSelectionSnapshot } from '@/test-utils';
import { useMutationStore } from '@/stores/mutation';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { executeSkillRemoval, openSkillRemoval } from '../skill-remove';
import { executeManageAgentChanges } from '../skill-manage-agents';
import { executeSkillCopy } from '../skill-copy';

const mocks = vi.hoisted(() => ({
  previewRemove: vi.fn(),
  removeSkill: vi.fn(),
  previewManageSkillAgents: vi.fn(),
  getManageAgentSelection: vi.fn(),
  getCopyAgentSelection: vi.fn(),
  manageSkillAgents: vi.fn(),
  previewCopySkillToProjects: vi.fn(),
  copySkillToProjects: vi.fn(),
  syncSkills: vi.fn(),
  refreshContext: vi.fn(),
  deselectSkill: vi.fn(),
  getInstallWizardSession: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  previewRemove: (...args: unknown[]) => mocks.previewRemove(...args),
  removeSkill: (...args: unknown[]) => mocks.removeSkill(...args),
  previewManageSkillAgents: (...args: unknown[]) => mocks.previewManageSkillAgents(...args),
  getManageAgentSelection: (...args: unknown[]) => mocks.getManageAgentSelection(...args),
  getCopyAgentSelection: (...args: unknown[]) => mocks.getCopyAgentSelection(...args),
  manageSkillAgents: (...args: unknown[]) => mocks.manageSkillAgents(...args),
  previewCopySkillToProjects: (...args: unknown[]) => mocks.previewCopySkillToProjects(...args),
  copySkillToProjects: (...args: unknown[]) => mocks.copySkillToProjects(...args),
  getInstallWizardSession: () => mocks.getInstallWizardSession(),
}));

vi.mock('@/stores/skills-data', () => ({
  useSkillsDataStore: {
    getState: () => ({
      syncSkills: mocks.syncSkills,
      refreshContext: mocks.refreshContext,
    }),
  },
}));

vi.mock('@/stores/skill-detail', () => ({
  useSkillDetailStore: {
    getState: () => ({ selectedSkillRef: null, deselectSkill: mocks.deselectSkill }),
  },
}));

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

const context: SkillLocationRef = {
  environment: { kind: 'wsl', distro_name: 'Ubuntu' },
  scope: { scope: 'project', project_id: 'source' },
};

const skill = {
  name: 'toolkit',
  description: '',
  path: '/skills/toolkit',
  canonicalPath: '/canonical/toolkit',
  scope: 'project',
  agents: ['codex'],
  associatedAgents: ['codex'],
  hasUpdate: false,
} as InstalledSkill;

const token = {
  generation: 'preview-1',
  registryRevision: 'registry-1',
  environmentRevision: 'environment-1',
  contextRevision: 'context-1',
};

const recoveryAction = {
  resourceId: 'recovery-1',
  suggestedActionCode: 'reviewChanges',
} as const;

const removePreview = {
  token,
  context,
  skillName: skill.name,
  canonical: 'directory',
  physicalEntries: [],
} as RemovePreview;

const managePreview = {
  token,
  context,
  skillName: skill.name,
  canonicalPayload: null,
  confirmation: null,
} satisfies ManageAgentsPreview;

const manageSnapshot: ManageAgentSelectionSnapshot = {
  selection: makeAgentSelectionSnapshot({ revision: 'manage-selection-1' }),
  optionStates: [],
};

const manageSubmission: AgentSelectionSubmission = {
  revision: 'manage-selection-1',
  selectedOptionIds: [],
  requestedMode: 'copy',
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

describe('skill workflows', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.previewRemove.mockReset();
    mocks.previewManageSkillAgents.mockReset();
    mocks.getManageAgentSelection.mockReset();
    mocks.getCopyAgentSelection.mockReset();
    useMutationStore.setState({ activeMutation: null, loading: false, cancelling: false });
    useInstallWizardSessionStore.setState({
      revision: 0, active: false, loading: false, hasConfirmedSnapshot: false,
      syncError: null, monitorRetryRevision: 0, snapshotVersion: 0,
    });
    mocks.getInstallWizardSession.mockResolvedValue({ revision: 1, active: true });
    useSkillDialogStore.setState({
      deleteTarget: null,
      deletePreview: null,
      deleteFeedback: null,
      loadingAgentDetails: false,
      manageAgentsSkill: null,
      manageAgentsContext: undefined,
      copySkill: null,
      copyContext: undefined,
    });
    mocks.removeSkill.mockResolvedValue({ units: [{ status: 'succeeded' }] });
    mocks.previewRemove.mockResolvedValue(removePreview);
    mocks.getManageAgentSelection.mockResolvedValue(manageSnapshot);
    mocks.getCopyAgentSelection.mockResolvedValue({
      selection: makeAgentSelectionSnapshot({ revision: manageSubmission.revision }),
    });
    mocks.previewManageSkillAgents.mockResolvedValue({ status: 'ready', preview: managePreview });
    mocks.manageSkillAgents.mockResolvedValue({ units: [{ status: 'succeeded', error: null }] });
    mocks.previewCopySkillToProjects.mockResolvedValue({
      status: 'ready',
      preview: {
        token,
        payload: {},
        source: context,
        targetEnvironment: { kind: 'native' },
        targets: [],
      },
    });
    mocks.copySkillToProjects.mockResolvedValue({ units: [{ status: 'succeeded' }] });
  });

  it('executes removal from the preview captured by dialog state', async () => {
    useSkillDialogStore.setState({
      deleteTarget: { skill, scope: 'project', projectPath: '/source', context },
      deletePreview: removePreview,
    });

    await executeSkillRemoval();

    expect(mocks.removeSkill).toHaveBeenCalledWith({
      token,
      context,
      skillName: skill.name,
      intent: { kind: 'fullSkill' },
    });
    expect(useSkillDialogStore.getState().deleteTarget).toBeNull();
  });

  it('returns notRun without local failure feedback when installation wins removal admission', async () => {
    useSkillDialogStore.setState({
      deleteTarget: { skill, scope: 'project', projectPath: '/source', context },
      deletePreview: removePreview,
    });
    mocks.removeSkill.mockRejectedValueOnce({ kind: 'installWizardActive' });

    await expect(executeSkillRemoval()).resolves.toEqual({ status: 'notRun' });

    expect(useSkillDialogStore.getState().deleteFeedback).toBeNull();
    expect(mocks.syncSkills).not.toHaveBeenCalled();
  });

  it('does not let a slow removal preview replace a newer dialog target', async () => {
    const first = deferred<RemovePreview>();
    const second = deferred<RemovePreview>();
    const otherSkill = { ...skill, name: 'other-toolkit' };
    const otherPreview = { ...removePreview, skillName: otherSkill.name };
    mocks.previewRemove
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    const firstOpen = openSkillRemoval(skill, context, '/source');
    const secondOpen = openSkillRemoval(otherSkill, context, '/source');
    second.resolve(otherPreview);
    await secondOpen;

    expect(useSkillDialogStore.getState().deleteTarget?.skill.name).toBe(otherSkill.name);
    expect(useSkillDialogStore.getState().deletePreview?.skillName).toBe(otherSkill.name);

    first.resolve(removePreview);
    await firstOpen;
    expect(useSkillDialogStore.getState().deleteTarget?.skill.name).toBe(otherSkill.name);
    expect(useSkillDialogStore.getState().deletePreview?.skillName).toBe(otherSkill.name);
  });

  it('keeps the removal dialog open when preview loading fails', async () => {
    mocks.previewRemove.mockRejectedValueOnce({ kind: 'staleTarget' });

    await openSkillRemoval(skill, context, '/source');

    expect(useSkillDialogStore.getState().deleteTarget?.skill.name).toBe(skill.name);
    expect(useSkillDialogStore.getState().deletePreview).toBeNull();
    expect(useSkillDialogStore.getState().deleteFeedback).toBe('previewError');
    expect(useSkillDialogStore.getState().loadingAgentDetails).toBe(false);
  });

  it('keeps the removal preview available when execution fails', async () => {
    useSkillDialogStore.setState({
      deleteTarget: { skill, scope: 'project', projectPath: '/source', context },
      deletePreview: removePreview,
    });
    mocks.removeSkill.mockResolvedValueOnce({
      units: [{ status: 'failed', error: null }],
    });

    await executeSkillRemoval();

    expect(useSkillDialogStore.getState().deleteTarget?.skill.name).toBe(skill.name);
    expect(useSkillDialogStore.getState().deletePreview).toBe(removePreview);
    expect(useSkillDialogStore.getState().deleteFeedback).toBe('executionError');
  });

  it('returns removal recovery actions without turning them into a retryable execution error', async () => {
    useSkillDialogStore.setState({
      deleteTarget: { skill, scope: 'project', projectPath: '/source', context },
      deletePreview: removePreview,
    });
    mocks.removeSkill.mockResolvedValueOnce({
      units: [{
        status: 'recoveryRequired',
        retryable: false,
        recovery: recoveryAction,
        error: null,
      }],
    });

    const outcome = await executeSkillRemoval();

    expect(outcome).toEqual({
      status: 'recoveryRequired',
      recovery: [recoveryAction],
    });
    expect(useSkillDialogStore.getState().deleteTarget?.skill.name).toBe(skill.name);
    expect(useSkillDialogStore.getState().deletePreview).toBe(removePreview);
    expect(useSkillDialogStore.getState().deleteFeedback).toBeNull();
  });

  it('reloads the removal preview when execution reports stale scope', async () => {
    const refreshedPreview = {
      ...removePreview,
      token: { ...token, generation: 'preview-2' },
    } as RemovePreview;
    useSkillDialogStore.setState({
      deleteTarget: { skill, scope: 'project', projectPath: '/source', context },
      deletePreview: removePreview,
    });
    mocks.removeSkill.mockResolvedValueOnce({
      units: [{
        status: 'failed',
        error: {
          code: 'staleTarget',
          parameters: {},
          field: null,
          severity: 'error',
          retryable: true,
          technicalDetails: null,
          environment: null,
          context: null,
          unitId: 'remove:toolkit',
          recoveryResourceId: null,
          displayPaths: [],
        },
      }],
    });
    mocks.previewRemove.mockResolvedValueOnce(refreshedPreview);

    await executeSkillRemoval();

    expect(mocks.previewRemove).toHaveBeenCalledWith(context, skill.name);
    expect(useSkillDialogStore.getState().deletePreview).toBe(refreshedPreview);
    expect(useSkillDialogStore.getState().deleteFeedback).toBe('stale');
  });

  it('submits the Backend selection expectation unchanged', async () => {
    useSkillDialogStore.setState({
      manageAgentsSkill: skill,
      manageAgentsContext: context,
    });

    await executeManageAgentChanges(manageSubmission);

    expect(mocks.previewManageSkillAgents).toHaveBeenCalledWith({
      context,
      skillName: skill.name,
      agentSelection: manageSubmission,
    });
    expect(mocks.manageSkillAgents).toHaveBeenCalledWith(expect.objectContaining({
      context,
      skillName: skill.name,
      agentSelection: manageSubmission,
      confirmEntityDirectories: false,
    }));
  });

  it('requires structured confirmation before removing entity directories', async () => {
    mocks.previewManageSkillAgents.mockResolvedValueOnce({
      status: 'ready',
      preview: { ...managePreview, confirmation: { removesEntityDirectories: true } },
    });
    useSkillDialogStore.setState({ manageAgentsSkill: skill, manageAgentsContext: context });

    await expect(executeManageAgentChanges(manageSubmission)).resolves.toEqual({ status: 'confirmationRequired' });
    expect(mocks.manageSkillAgents).not.toHaveBeenCalled();
  });

  it('keeps a management recovery action separate from an ordinary failure', async () => {
    useSkillDialogStore.setState({ manageAgentsSkill: skill, manageAgentsContext: context });
    mocks.manageSkillAgents.mockResolvedValueOnce({
      units: [{ status: 'recoveryRequired', recovery: recoveryAction, error: null }],
    });

    await expect(executeManageAgentChanges(manageSubmission)).resolves.toEqual({
      status: 'recoveryRequired',
      response: { units: [{ status: 'recoveryRequired', recovery: recoveryAction, error: null }] },
      recovery: [recoveryAction],
    });
    expect(mocks.syncSkills).not.toHaveBeenCalled();
  });

  it('returns a failed outcome and keeps the management dialog open on unit failure', async () => {
    useSkillDialogStore.setState({
      manageAgentsSkill: skill,
      manageAgentsContext: context,
    });
    mocks.manageSkillAgents.mockResolvedValueOnce({ units: [{ status: 'failed', error: null }] });

    await expect(executeManageAgentChanges(manageSubmission)).resolves.toEqual({ status: 'failed' });
    expect(useSkillDialogStore.getState().manageAgentsSkill).toBe(skill);
    expect(mocks.syncSkills).not.toHaveBeenCalled();
  });

  it('publishes the latest snapshot when preview reports an expired revision', async () => {
    const latest = { ...manageSnapshot, selection: { ...manageSnapshot.selection, revision: 'manage-selection-2' } };
    useSkillDialogStore.setState({ manageAgentsSkill: skill, manageAgentsContext: context });
    mocks.previewManageSkillAgents.mockResolvedValueOnce({ status: 'selectionStale', snapshot: latest });

    await expect(executeManageAgentChanges(manageSubmission)).resolves.toEqual({
      status: 'stale',
      snapshot: latest,
    });
    expect(mocks.manageSkillAgents).not.toHaveBeenCalled();
  });

  it('copies to project IDs in one explicitly selected target Environment', async () => {
    useSkillDialogStore.setState({ copySkill: skill, copyContext: context });
    mocks.copySkillToProjects.mockResolvedValueOnce({
      units: [{
        status: 'succeeded',
        target: { scope: { scope: 'project', project_id: 'native-target' } },
      }],
    });

    const outcome = await executeSkillCopy({
      environment: { kind: 'native' },
      projectIds: ['native-target'],
      agentSelection: manageSubmission,
    });

    expect(outcome.status).toBe('succeeded');
    expect(mocks.previewCopySkillToProjects).toHaveBeenCalledWith(expect.objectContaining({
      source: context,
      targetEnvironment: { kind: 'native' },
      targetProjectIds: ['native-target'],
      agentSelection: manageSubmission,
    }));
    expect(mocks.copySkillToProjects).toHaveBeenCalledWith(expect.objectContaining({ token }));
    expect(mocks.refreshContext).toHaveBeenCalledWith({
      environment: { kind: 'native' },
      scope: { scope: 'project', project_id: 'native-target' },
    }, { origin: 'selfMutation', mutatedSkillNames: ['toolkit'] });
  });

  it('returns blocked without a copy error when installation wins copy admission', async () => {
    useSkillDialogStore.setState({ copySkill: skill, copyContext: context });
    mocks.copySkillToProjects.mockRejectedValueOnce({ kind: 'installWizardActive' });

    await expect(executeSkillCopy({
      environment: { kind: 'native' },
      projectIds: ['native-target'],
      agentSelection: manageSubmission,
    })).resolves.toEqual({ status: 'blocked' });

    expect(mocks.refreshContext).not.toHaveBeenCalled();
  });

  it('returns the latest Agent selection without starting copy execution', async () => {
    useSkillDialogStore.setState({ copySkill: skill, copyContext: context });
    const selection = makeAgentSelectionSnapshot({ revision: 'copy-selection-2' });
    mocks.previewCopySkillToProjects.mockResolvedValue({
      status: 'selectionStale',
      snapshot: { selection },
    });

    await expect(executeSkillCopy({
      environment: { kind: 'native' },
      projectIds: ['native-target'],
      agentSelection: manageSubmission,
    })).resolves.toEqual({
      status: 'selectionStale',
      snapshot: { selection },
    });
    expect(mocks.copySkillToProjects).not.toHaveBeenCalled();
  });

  it('loads the latest Agent selection when execution discovers that the preview is stale', async () => {
    useSkillDialogStore.setState({ copySkill: skill, copyContext: context });
    const selection = makeAgentSelectionSnapshot({ revision: 'copy-selection-new' });
    mocks.copySkillToProjects.mockRejectedValueOnce({ kind: 'staleContext' });
    mocks.getCopyAgentSelection.mockResolvedValueOnce({ selection });

    await expect(executeSkillCopy({
      environment: { kind: 'native' },
      projectIds: ['native-target'],
      agentSelection: manageSubmission,
    })).resolves.toEqual({
      status: 'selectionStale',
      snapshot: { selection },
    });
    expect(mocks.getCopyAgentSelection).toHaveBeenCalledWith(context, 'toolkit');
  });

  it.each(['staleRegistry', 'staleEnvironment'] as const)(
    'checks the latest Agent selection after a %s execution error',
    async (kind) => {
      useSkillDialogStore.setState({ copySkill: skill, copyContext: context });
      const selection = makeAgentSelectionSnapshot({ revision: `copy-selection-${kind}` });
      mocks.copySkillToProjects.mockRejectedValueOnce({ kind });
      mocks.getCopyAgentSelection.mockResolvedValueOnce({ selection });

      await expect(executeSkillCopy({
        environment: { kind: 'native' },
        projectIds: ['native-target'],
        agentSelection: manageSubmission,
      })).resolves.toEqual({
        status: 'selectionStale',
        snapshot: { selection },
      });
    },
  );

  it('returns retryable project IDs for partial copy outcomes without closing the dialog', async () => {
    useSkillDialogStore.setState({ copySkill: skill, copyContext: context });
    mocks.copySkillToProjects.mockResolvedValue({
      units: [
        { status: 'succeeded', target: { scope: { scope: 'project', project_id: 'project-b' } } },
        { status: 'failed', retryable: true, target: { scope: { scope: 'project', project_id: 'project-c' } } },
      ],
    });

    const outcome = await executeSkillCopy({
      environment: { kind: 'native' },
      projectIds: ['project-b', 'project-c'],
      agentSelection: manageSubmission,
    });

    expect(outcome).toMatchObject({
      status: 'partial',
      succeededProjectIds: ['project-b'],
      retryableProjectIds: ['project-c'],
    });
    expect(mocks.refreshContext).toHaveBeenCalledTimes(1);
    expect(mocks.refreshContext).toHaveBeenCalledWith({
      environment: { kind: 'native' },
      scope: { scope: 'project', project_id: 'project-b' },
    }, { origin: 'selfMutation', mutatedSkillNames: ['toolkit'] });
    expect(useSkillDialogStore.getState().copySkill).toBe(skill);
  });

  it('preserves a single project mutation result without converting its error report', async () => {
    useSkillDialogStore.setState({ copySkill: skill, copyContext: context });
    const failedUnit = {
      status: 'failed' as const,
      retryable: false,
      error: { code: 'configurationCorrupted' },
      target: { scope: { scope: 'project' as const, project_id: 'project-c' } },
    };
    mocks.copySkillToProjects.mockResolvedValue({ units: [failedUnit] });

    const outcome = await executeSkillCopy({
      environment: { kind: 'native' },
      projectIds: ['project-c'],
      agentSelection: manageSubmission,
    });

    expect(outcome).toEqual({ status: 'failed', unit: failedUnit });
  });

  it('returns recoveryRequired directly for a single project', async () => {
    useSkillDialogStore.setState({ copySkill: skill, copyContext: context });
    mocks.copySkillToProjects.mockResolvedValue({
      units: [{
        status: 'recoveryRequired',
        retryable: false,
        recovery: recoveryAction,
        target: { scope: { scope: 'project', project_id: 'project-c' } },
      }],
    });

    const outcome = await executeSkillCopy({
      environment: { kind: 'native' },
      projectIds: ['project-c'],
      agentSelection: manageSubmission,
    });

    expect(outcome).toMatchObject({
      status: 'recoveryRequired',
      succeededProjectIds: [],
      recovery: [recoveryAction],
    });
  });

  it('keeps copy recovery in a multi-project partial outcome without making it retryable', async () => {
    useSkillDialogStore.setState({ copySkill: skill, copyContext: context });
    mocks.copySkillToProjects.mockResolvedValue({
      units: [
        { status: 'succeeded', target: { scope: { scope: 'project', project_id: 'project-b' } } },
        {
          status: 'recoveryRequired',
          retryable: false,
          recovery: recoveryAction,
          target: { scope: { scope: 'project', project_id: 'project-c' } },
        },
      ],
    });

    const outcome = await executeSkillCopy({
      environment: { kind: 'native' },
      projectIds: ['project-b', 'project-c'],
      agentSelection: manageSubmission,
    });

    expect(outcome).toMatchObject({
      status: 'partial',
      succeededProjectIds: ['project-b'],
      failedProjectIds: ['project-c'],
      retryableProjectIds: [],
      recovery: [recoveryAction],
    });
    expect(useSkillDialogStore.getState().copySkill).toBe(skill);
  });

});
