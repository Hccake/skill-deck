import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { SkillLocationRef, InstalledSkill } from '@/bindings';
import { useMutationStore } from '../mutation';
import { projectWorkspace } from '../projects';
import { useSkillDialogStore } from '../skill-dialog';
import { useInstallWizardSessionStore } from '../install-wizard-session';

const mocks = vi.hoisted(() => ({
  previewRemove: vi.fn(),
  removeSkill: vi.fn(),
  previewCopySkillToProjects: vi.fn(),
  copySkillToProjects: vi.fn(),
  openInstallWizard: vi.fn(),
  previewManageSkillAgents: vi.fn(),
  manageSkillAgents: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  previewRemove: (...args: unknown[]) => mocks.previewRemove(...args),
  removeSkill: (...args: unknown[]) => mocks.removeSkill(...args),
  previewCopySkillToProjects: (...args: unknown[]) => mocks.previewCopySkillToProjects(...args),
  copySkillToProjects: (...args: unknown[]) => mocks.copySkillToProjects(...args),
  openInstallWizard: (...args: unknown[]) => mocks.openInstallWizard(...args),
  previewManageSkillAgents: (...args: unknown[]) => mocks.previewManageSkillAgents(...args),
  manageSkillAgents: (...args: unknown[]) => mocks.manageSkillAgents(...args),
}));

vi.mock('../skills-data', () => ({
  useSkillsDataStore: { getState: () => ({ syncSkills: vi.fn(async () => undefined) }) },
}));

vi.mock('../skill-detail', () => ({
  useSkillDetailStore: { getState: () => ({ selectedSkillRef: null, deselectSkill: vi.fn() }) },
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

describe('Skill dialog context capture', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    useMutationStore.setState({ activeMutation: null, loading: false, cancelling: false });
    useInstallWizardSessionStore.setState({ revision: 0, active: false, loading: false });
    useSkillDialogStore.setState({
      deleteTarget: null,
      deletePreview: null,
      deleteFeedback: null,
      loadingAgentDetails: false,
    });
    vi.spyOn(projectWorkspace, 'getSnapshot').mockReturnValue({
      environment: context.environment,
      phase: 'ready',
      projects: [
        { binding: { id: 'source', nativePath: '/source', displayName: null, order: null }, storage: { access: 'native', owner: context.environment } },
        { binding: { id: 'target', nativePath: '/target', displayName: null, order: null }, storage: { access: 'native', owner: context.environment } },
      ],
      error: null,
      completeness: 'complete',
      environmentRevision: 1,
      lastAttemptAt: 1,
      lastSuccessAt: 1,
      freshUntil: 300_001,
      version: 1,
    });
    mocks.previewRemove.mockResolvedValue({
      token,
      context,
      skillName: 'toolkit',
      standard: 'directory',
      physicalEntries: [],
      restoresLibrary: false,
    });
    mocks.removeSkill.mockResolvedValue({ units: [{ status: 'succeeded' }] });
    mocks.previewCopySkillToProjects.mockResolvedValue({
      token,
      payload: {},
      source: context,
      targetEnvironment: context.environment,
      targets: [],
    });
    mocks.copySkillToProjects.mockResolvedValue({ units: [{ status: 'succeeded' }] });
    mocks.manageSkillAgents.mockResolvedValue({ units: [{ status: 'succeeded', error: null }] });
  });

  it('captures removal dialog state without owning preview orchestration', () => {
    useSkillDialogStore.getState().openDelete(skill, context, '/source');

    expect(useSkillDialogStore.getState().deleteTarget?.context).toEqual(context);
    expect(useSkillDialogStore.getState().deletePreview).toBeNull();
    expect(mocks.previewRemove).not.toHaveBeenCalled();
  });

  it('clears deletion feedback when opening or closing the dialog', () => {
    useSkillDialogStore.setState({ deleteFeedback: 'executionError' });

    useSkillDialogStore.getState().openDelete(skill, context, '/source');
    expect(useSkillDialogStore.getState().deleteFeedback).toBeNull();

    useSkillDialogStore.getState().setDeleteFeedback('stale');
    useSkillDialogStore.getState().closeDelete();
    expect(useSkillDialogStore.getState().deleteFeedback).toBeNull();
  });

  it('captures the source context when opening copy to project', () => {
    useSkillDialogStore.getState().openCopyToProject(skill, context);

    expect(useSkillDialogStore.getState().copyContext).toEqual(context);
  });

  it('captures Agent-management dialog state without owning preview orchestration', () => {
    useSkillDialogStore.getState().openManageAgents(skill, context);

    expect(useSkillDialogStore.getState().manageAgentsContext).toEqual(context);
    expect(useSkillDialogStore.getState().manageAgentsSkill).toBe(skill);
    expect(mocks.previewManageSkillAgents).not.toHaveBeenCalled();
  });

  it('does not open a second install wizard while one session is active', () => {
    useInstallWizardSessionStore.setState({ revision: 1, active: true });

    useSkillDialogStore.getState().openAdd(context, '/source');

    expect(mocks.openInstallWizard).not.toHaveBeenCalled();
  });
});
