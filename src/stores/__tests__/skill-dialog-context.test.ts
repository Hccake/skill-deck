import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ContextRef, InstalledSkill } from '@/bindings';
import { useMutationStore } from '../mutation';
import { useProjectStore } from '../projects';
import { useWorkspaceContextStore } from '../workspace-context';
import { useSkillDialogStore } from '../skill-dialog';

const mocks = vi.hoisted(() => ({
  openInstallWizard: vi.fn(),
  getSkillAgentDetails: vi.fn(),
  removeSkill: vi.fn(),
  manageSkillAgents: vi.fn(),
  cleanupDuplicateAgentCopies: vi.fn(),
  copySkillToProjects: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  openInstallWizard: (...args: unknown[]) => mocks.openInstallWizard(...args),
  getSkillAgentDetails: (...args: unknown[]) => mocks.getSkillAgentDetails(...args),
  removeSkill: (...args: unknown[]) => mocks.removeSkill(...args),
  manageSkillAgents: (...args: unknown[]) => mocks.manageSkillAgents(...args),
  cleanupDuplicateAgentCopies: (...args: unknown[]) => mocks.cleanupDuplicateAgentCopies(...args),
  copySkillToProjects: (...args: unknown[]) => mocks.copySkillToProjects(...args),
}));

vi.mock('../skill-detail', () => ({
  useSkillDetailStore: {
    getState: () => ({ selectedSkillRef: null, deselectSkill: vi.fn() }),
  },
}));

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

const ubuntuProject: ContextRef = {
  environment: { kind: 'wsl', distro_name: 'Ubuntu' },
  scope: { scope: 'project', project_id: 'source' },
};
const debianGlobal: ContextRef = {
  environment: { kind: 'wsl', distro_name: 'Debian' },
  scope: { scope: 'global' },
};

function skill(): InstalledSkill {
  return {
    name: 'toolkit',
    description: '',
    path: '/skills/toolkit',
    canonicalPath: '/canonical/toolkit',
    scope: 'project',
    agents: [],
    hasUpdate: false,
  };
}

describe('Skill dialog context capture', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useMutationStore.setState({ activeMutation: null, loading: false, cancelling: false });
    useWorkspaceContextStore.setState({ selectedContext: ubuntuProject });
    useProjectStore.setState({
      projectsByEnvironment: {
        'wsl:Ubuntu': [
          {
            binding: {
              id: 'source',
              nativePath: '/home/me/source',
              displayName: null,
              order: null,
              suppressCrossStorageWarning: false,
            },
            storage: { access: 'native', owner: ubuntuProject.environment },
          },
          {
            binding: {
              id: 'target',
              nativePath: '/home/me/target',
              displayName: null,
              order: null,
              suppressCrossStorageWarning: false,
            },
            storage: { access: 'native', owner: ubuntuProject.environment },
          },
        ],
      },
    });
    useSkillDialogStore.setState({
      deleteTarget: null,
      manageAgentsSkill: null,
      manageAgentsContext: undefined,
      copySkill: null,
      copyContext: undefined,
    });
    mocks.getSkillAgentDetails.mockResolvedValue({});
    mocks.removeSkill.mockResolvedValue(undefined);
    mocks.copySkillToProjects.mockResolvedValue({ results: [] });
    mocks.openInstallWizard.mockResolvedValue(undefined);
  });

  it('uses the context captured when delete opens even after workspace context changes', async () => {
    useSkillDialogStore.getState().openDelete(skill(), ubuntuProject, '/home/me/source');
    useWorkspaceContextStore.setState({ selectedContext: debianGlobal });

    await useSkillDialogStore.getState().deleteSkill({ fullRemoval: true });

    expect(mocks.getSkillAgentDetails).toHaveBeenCalledWith(ubuntuProject, 'toolkit');
    expect(mocks.removeSkill).toHaveBeenCalledWith(ubuntuProject, {
      name: 'toolkit',
      fullRemoval: true,
      agents: undefined,
      agentTargets: undefined,
    });
  });

  it('opens the wizard and copies with the explicitly captured source context', async () => {
    useSkillDialogStore.getState().openAdd(ubuntuProject, '/home/me/source');
    useSkillDialogStore.getState().openCopyToProject(skill(), ubuntuProject);
    useWorkspaceContextStore.setState({ selectedContext: debianGlobal });

    await useSkillDialogStore.getState().executeCopy(['/home/me/target']);

    expect(mocks.openInstallWizard).toHaveBeenCalledWith(expect.objectContaining({
      context: ubuntuProject,
      projectPath: '/home/me/source',
    }));
    expect(mocks.copySkillToProjects).toHaveBeenCalledWith(expect.objectContaining({
      source: ubuntuProject,
      targets: [{
        environment: ubuntuProject.environment,
        scope: { scope: 'project', project_id: 'target' },
      }],
    }));
  });
});
