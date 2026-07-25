import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ContextRef, InstalledSkill, ManageAgentsPreview, RemovePreview } from '@/bindings';
import { useMutationStore } from '@/stores/mutation';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { executeSkillRemoval, openSkillRemoval } from '../skill-remove';
import { executeManageAgentChanges, openManageAgentChanges } from '../skill-manage-agents';
import { executeSkillCopy } from '../skill-copy';
import { executeDuplicateCleanup } from '../duplicate-cleanup';

const mocks = vi.hoisted(() => ({
  previewRemove: vi.fn(),
  removeSkill: vi.fn(),
  previewManageSkillAgents: vi.fn(),
  manageSkillAgents: vi.fn(),
  previewCopySkillToProjects: vi.fn(),
  copySkillToProjects: vi.fn(),
  cleanupDuplicateAgentCopies: vi.fn(),
  syncSkills: vi.fn(),
  deselectSkill: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  previewRemove: (...args: unknown[]) => mocks.previewRemove(...args),
  removeSkill: (...args: unknown[]) => mocks.removeSkill(...args),
  previewManageSkillAgents: (...args: unknown[]) => mocks.previewManageSkillAgents(...args),
  manageSkillAgents: (...args: unknown[]) => mocks.manageSkillAgents(...args),
  previewCopySkillToProjects: (...args: unknown[]) => mocks.previewCopySkillToProjects(...args),
  copySkillToProjects: (...args: unknown[]) => mocks.copySkillToProjects(...args),
  cleanupDuplicateAgentCopies: (...args: unknown[]) => mocks.cleanupDuplicateAgentCopies(...args),
}));

vi.mock('@/stores/skills-data', () => ({
  useSkillsDataStore: { getState: () => ({ syncSkills: mocks.syncSkills }) },
}));

vi.mock('@/stores/skill-detail', () => ({
  useSkillDetailStore: {
    getState: () => ({ selectedSkillRef: null, deselectSkill: mocks.deselectSkill }),
  },
}));

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

const context: ContextRef = {
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
  availableAgents: [],
  selectionGroups: { global: [], project: [] },
  observedEntries: [{
    entryId: 'shared-entry',
    displayPath: { environment: context.environment, nativePath: '/shared-entry' },
    kind: 'directory',
    physicalTargetKey: 'wsl:/shared-entry',
    owners: [{ agentId: 'codex', displayName: 'Codex', logicalTargetId: 'codex-private' }],
    willBreakIfCanonicalRemoved: false,
  }],
  canonicalPayload: null,
  addTargets: [],
} as ManageAgentsPreview;

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
    useMutationStore.setState({ activeMutation: null, loading: false, cancelling: false });
    useSkillDialogStore.setState({
      deleteTarget: null,
      deletePreview: null,
      deleteFeedback: null,
      loadingAgentDetails: false,
      manageAgentsSkill: null,
      manageAgentsContext: undefined,
      manageAgentDetails: null,
      copySkill: null,
      copyContext: undefined,
    });
    mocks.removeSkill.mockResolvedValue({ units: [{ status: 'succeeded' }] });
    mocks.previewRemove.mockResolvedValue(removePreview);
    mocks.previewManageSkillAgents.mockResolvedValue(managePreview);
    mocks.manageSkillAgents.mockResolvedValue({ units: [{ status: 'succeeded', error: null }] });
    mocks.previewCopySkillToProjects.mockResolvedValue({
      token,
      payload: {},
      source: context,
      targetEnvironment: { kind: 'host' },
      targets: [],
    });
    mocks.copySkillToProjects.mockResolvedValue({ units: [{ status: 'succeeded' }] });
    mocks.cleanupDuplicateAgentCopies.mockResolvedValue([
      { agent: 'codex', success: true, skipped: false, path: null, error: null },
    ]);
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

  it('passes Backend-owned physical entry removals through unchanged', async () => {
    useSkillDialogStore.setState({
      manageAgentsSkill: skill,
      manageAgentsContext: context,
      manageAgentDetails: managePreview,
    });

    await executeManageAgentChanges([], ['shared-entry'], 'copy', []);

    expect(mocks.previewManageSkillAgents).toHaveBeenCalledWith(expect.objectContaining({
      context,
      skillName: skill.name,
      removeEntryIds: ['shared-entry'],
    }));
    expect(mocks.manageSkillAgents).toHaveBeenCalledWith(expect.objectContaining({
      removeEntryIds: ['shared-entry'],
      confirmEntityDirectories: true,
    }));
  });

  it('returns a failed outcome and keeps the management dialog open when execution fails', async () => {
    useSkillDialogStore.setState({
      manageAgentsSkill: skill,
      manageAgentsContext: context,
      manageAgentDetails: managePreview,
    });
    mocks.manageSkillAgents.mockResolvedValueOnce({
      units: [{ status: 'failed', error: null }],
    });

    const outcome = await executeManageAgentChanges([], ['shared-entry'], 'copy', []);

    expect(outcome).toEqual({ status: 'failed' });
    expect(useSkillDialogStore.getState().manageAgentsSkill).toBe(skill);
    expect(useSkillDialogStore.getState().manageAgentDetails).toBe(managePreview);
    expect(mocks.syncSkills).toHaveBeenCalledWith(context);
  });

  it('keeps a management recovery action separate from an ordinary failure', async () => {
    useSkillDialogStore.setState({
      manageAgentsSkill: skill,
      manageAgentsContext: context,
      manageAgentDetails: managePreview,
    });
    mocks.manageSkillAgents.mockResolvedValueOnce({
      units: [{ status: 'recoveryRequired', recovery: recoveryAction, error: null }],
    });

    const outcome = await executeManageAgentChanges([], ['shared-entry'], 'copy', []);

    expect(outcome).toEqual({
      status: 'recoveryRequired',
      response: { units: [{ status: 'recoveryRequired', recovery: recoveryAction, error: null }] },
      recovery: [recoveryAction],
    });
    expect(mocks.syncSkills).not.toHaveBeenCalled();
  });

  it('returns a failed outcome when an atomic Agent change reports mixed unit results', async () => {
    useSkillDialogStore.setState({
      manageAgentsSkill: skill,
      manageAgentsContext: context,
      manageAgentDetails: managePreview,
    });
    mocks.manageSkillAgents.mockResolvedValueOnce({
      units: [
        { status: 'succeeded', error: null },
        { status: 'failed', error: null },
      ],
    });

    const outcome = await executeManageAgentChanges([], ['shared-entry'], 'copy', []);

    expect(outcome).toEqual({ status: 'failed' });
    expect(useSkillDialogStore.getState().manageAgentsSkill).toBe(skill);
    expect(mocks.syncSkills).toHaveBeenCalledWith(context);
  });

  it('refreshes the management preview when execution reports a stale scope', async () => {
    const refreshedPreview = {
      ...managePreview,
      token: { ...token, generation: 'manage-preview-2' },
    } as ManageAgentsPreview;
    useSkillDialogStore.setState({
      manageAgentsSkill: skill,
      manageAgentsContext: context,
      manageAgentDetails: managePreview,
    });
    mocks.manageSkillAgents.mockResolvedValueOnce({
      units: [{
        status: 'failed',
        error: {
          code: 'staleTarget',
          parameters: {},
          field: null,
          severity: 'error',
          retryable: true,
          technicalDetails: null,
          environment: context.environment,
          context,
          unitId: 'manage:toolkit',
          recoveryResourceId: null,
          displayPaths: [],
        },
      }],
    });
    mocks.previewManageSkillAgents
      .mockResolvedValueOnce(managePreview)
      .mockResolvedValueOnce(refreshedPreview);

    const outcome = await executeManageAgentChanges([], ['shared-entry'], 'copy', []);

    expect(outcome.status).toBe('stale');
    expect(mocks.previewManageSkillAgents).toHaveBeenLastCalledWith(expect.objectContaining({
      context,
      skillName: skill.name,
      add: [],
      removeEntryIds: [],
      requestedMode: 'copy',
    }));
    expect(useSkillDialogStore.getState().manageAgentDetails).toBe(refreshedPreview);
  });

  it('does not let a slow Agent-management preview replace a newer dialog target', async () => {
    const first = deferred<ManageAgentsPreview>();
    const second = deferred<ManageAgentsPreview>();
    const otherSkill = { ...skill, name: 'other-toolkit' };
    const otherPreview = { ...managePreview, skillName: otherSkill.name };
    mocks.previewManageSkillAgents
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    const firstOpen = openManageAgentChanges(skill, context, '/source');
    const secondOpen = openManageAgentChanges(otherSkill, context, '/source');
    second.resolve(otherPreview);
    await secondOpen;

    expect(useSkillDialogStore.getState().manageAgentsSkill?.name).toBe(otherSkill.name);
    expect(useSkillDialogStore.getState().manageAgentDetails?.skillName).toBe(otherSkill.name);

    first.resolve(managePreview);
    await firstOpen;
    expect(useSkillDialogStore.getState().manageAgentsSkill?.name).toBe(otherSkill.name);
    expect(useSkillDialogStore.getState().manageAgentDetails?.skillName).toBe(otherSkill.name);
  });

  it('copies to project IDs in one explicitly selected target Environment', async () => {
    useSkillDialogStore.setState({ copySkill: skill, copyContext: context });

    const outcome = await executeSkillCopy({ environment: { kind: 'host' }, projectIds: ['host-target'] });

    expect(outcome.status).toBe('succeeded');
    expect(mocks.previewCopySkillToProjects).toHaveBeenCalledWith(expect.objectContaining({
      source: context,
      targetEnvironment: { kind: 'host' },
      targetProjectIds: ['host-target'],
    }));
    expect(mocks.copySkillToProjects).toHaveBeenCalledWith(expect.objectContaining({ token }));
  });

  it('returns retryable project IDs for partial copy outcomes without closing the dialog', async () => {
    useSkillDialogStore.setState({ copySkill: skill, copyContext: context });
    mocks.copySkillToProjects.mockResolvedValue({
      units: [
        { status: 'succeeded', target: { scope: { scope: 'project', project_id: 'project-b' } } },
        { status: 'failed', retryable: true, target: { scope: { scope: 'project', project_id: 'project-c' } } },
      ],
    });

    const outcome = await executeSkillCopy({
      environment: { kind: 'host' },
      projectIds: ['project-b', 'project-c'],
    });

    expect(outcome).toMatchObject({
      status: 'partial',
      succeededProjectIds: ['project-b'],
      retryableProjectIds: ['project-c'],
    });
    expect(useSkillDialogStore.getState().copySkill).toBe(skill);
  });

  it('keeps copy recovery separate from ordinary retryable targets', async () => {
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
      environment: { kind: 'host' },
      projectIds: ['project-b', 'project-c'],
    });

    expect(outcome).toMatchObject({
      status: 'recoveryRequired',
      succeededProjectIds: ['project-b'],
      recovery: [recoveryAction],
    });
    expect(useSkillDialogStore.getState().copySkill).toBe(skill);
  });

  it('refreshes the management preview after duplicate cleanup', async () => {
    useSkillDialogStore.setState({
      manageAgentsSkill: skill,
      manageAgentsContext: context,
      manageAgentDetails: managePreview,
    });

    await executeDuplicateCleanup(['codex']);

    expect(mocks.cleanupDuplicateAgentCopies).toHaveBeenCalledWith(context, {
      skillName: skill.name,
      agents: ['codex'],
    });
    expect(useSkillDialogStore.getState().manageAgentDetails).toEqual(managePreview);
    expect(mocks.syncSkills).toHaveBeenCalledWith(context);
  });
});
