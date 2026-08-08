/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render as testingLibraryRender, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { EnvironmentInfo, EnvironmentRef, InstalledSkill, ProjectInfo } from '@/bindings';
import { environmentKey } from '@/lib/context';
import { makeAgentSelectionSnapshot } from '@/test-utils';
import { TooltipProvider } from '@/components/ui/tooltip';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { CopyToProjectDialogContainer } from '../CopyToProjectDialogContainer';

const native: EnvironmentInfo = {
  environment: { kind: 'native' },
  displayName: 'Windows',
  status: 'available',
  revision: 1,
  error: null,
};
const ubuntu: EnvironmentInfo = {
  environment: { kind: 'wsl', distro_name: 'Ubuntu' },
  displayName: 'Ubuntu',
  status: 'available',
  revision: 1,
  error: null,
};

function project(id: string, environment: EnvironmentRef): ProjectInfo {
  return {
    binding: {
      id,
      nativePath: environment.kind === 'native' ? `C:\\Code\\${id}` : `/work/${id}`,
      displayName: null,
      order: null,
      suppressCrossStorageWarning: false,
    },
    storage: {
      access: 'native',
      owner: environment.kind === 'native' ? null : environment,
    },
  };
}

const mocks = vi.hoisted(() => ({
  environments: [] as EnvironmentInfo[],
  projectsByEnvironment: {} as Record<string, ProjectInfo[]>,
  execute: vi.fn(),
  listSkills: vi.fn(),
  executeSkillCopy: vi.fn(),
  agentSelection: null as unknown,
  selectGlobal: vi.fn(),
  selectProject: vi.fn(),
  switchEnvironment: vi.fn(),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => (
      key === 'skills.copyToProject.description' ? `description:${options?.name}` : key
    ),
  }),
}));

vi.mock('@/stores/environment', () => ({
  useEnvironmentStore: (selector: (state: { environments: EnvironmentInfo[] }) => unknown) => (
    selector({ environments: mocks.environments })
  ),
}));

vi.mock('@/stores/projects', () => ({
  projectWorkspace: {
    execute: (...args: unknown[]) => mocks.execute(...args),
    getSnapshot: (environment: EnvironmentRef) => ({
      projects: mocks.projectsByEnvironment[environmentKey(environment)] ?? [],
    }),
  },
  projectSnapshotFor: (environment: EnvironmentRef) => ({
    projects: mocks.projectsByEnvironment[environmentKey(environment)] ?? [],
  }),
}));

vi.mock('@/hooks/useProjectWorkspace', () => ({
  useProjectCatalog: () => mocks.projectsByEnvironment,
}));

vi.mock('@/hooks/useCopyAgentSelection', () => ({
  useCopyAgentSelection: () => mocks.agentSelection,
}));

vi.mock('@/hooks/useTauriApi', () => ({
  listSkills: (...args: unknown[]) => mocks.listSkills(...args),
  openInstallWizard: vi.fn(),
}));

vi.mock('@/workflows/skill-copy', () => ({
  executeSkillCopy: (...args: unknown[]) => mocks.executeSkillCopy(...args),
}));

vi.mock('@/hooks/useBusinessWriteBlocked', () => ({
  useBusinessWriteBlocked: () => false,
  isBusinessWriteBlocked: () => false,
}));

vi.mock('@/stores/workspace-context', () => ({
  useWorkspaceContextStore: () => ({
    selectGlobal: mocks.selectGlobal,
    selectProject: mocks.selectProject,
    switchEnvironment: mocks.switchEnvironment,
  }),
}));

const sourceContext = {
  environment: native.environment,
  scope: { scope: 'project' as const, project_id: 'source' },
};
const skill: InstalledSkill = {
  name: 'toolkit',
  description: '',
  path: 'C:\\Code\\source\\.agents\\skills\\toolkit',
  canonicalPath: 'C:\\Code\\source\\.agents\\skills\\toolkit',
  scope: 'project',
  agents: ['claude-code'],
  associatedAgents: ['claude-code'],
  source: 'owner/repo',
};

function renderDialog() {
  return testingLibraryRender(<CopyToProjectDialogContainer />, { wrapper: TooltipProvider });
}

describe('CopyToProjectDialogContainer', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    Element.prototype.scrollIntoView = vi.fn();
    mocks.environments = [native, ubuntu];
    mocks.projectsByEnvironment = {
      native: [project('source', native.environment)],
      'wsl:ubuntu': [project('ubuntu-target', ubuntu.environment)],
    };
    mocks.execute.mockResolvedValue({ status: 'succeeded' });
    mocks.listSkills.mockResolvedValue({ skills: [] });
    mocks.agentSelection = {
      status: 'ready',
      snapshot: {
        selection: makeAgentSelectionSnapshot({
          revision: 'copy-container-selection',
          agents: [{
            kind: 'standard',
            id: 'claude-code',
            displayName: 'Claude Code',
            detection: 'detected',
            directoryAccess: 'privateOnly',
            installOptionId: 'claude',
            groupId: null,
          }],
          installOptions: [{
            id: 'claude',
            kind: 'standardDirectory',
            agentIds: ['claude-code'],
            displayName: 'Claude Code',
            path: '~/.claude/skills',
            groupId: null,
            selectable: true,
            modeConstraint: 'userSelectable',
            disabledReason: null,
          }],
          userModeOptionIds: ['claude'],
        }),
      },
      retry: vi.fn(),
    };
    useSkillDialogStore.getState().closeCopyToProject();
    useSkillDialogStore.getState().openCopyToProject(skill, sourceContext);
  });

  it('requires an explicit reselect after the target disappears without changing Context', async () => {
    const view = renderDialog();

    fireEvent.click(screen.getByRole('combobox', {
      name: 'skills.copyToProject.targetEnvironment',
    }));
    fireEvent.click(await screen.findByRole('option', { name: 'Ubuntu' }));
    fireEvent.click(await screen.findByText('/work/ubuntu-target'));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Claude Code' }));

    mocks.environments = [native];
    view.rerender(<CopyToProjectDialogContainer />);

    expect((await screen.findAllByText('skills.copyToProject.targetEnvironmentMissing')).length)
      .toBeGreaterThan(0);
    expect(screen.queryByText('/work/ubuntu-target')).toBeNull();
    expect((screen.getByRole('checkbox', { name: 'Claude Code' }) as HTMLButtonElement).dataset.state)
      .toBe('checked');

    mocks.environments = [ubuntu, native];
    view.rerender(<CopyToProjectDialogContainer />);
    expect(screen.queryByText('/work/ubuntu-target')).toBeNull();

    fireEvent.click(screen.getByRole('combobox', {
      name: 'skills.copyToProject.targetEnvironment',
    }));
    fireEvent.click(await screen.findByRole('option', { name: 'Ubuntu' }));
    expect(await screen.findByText('/work/ubuntu-target')).toBeDefined();
    expect((screen.getByRole('button', {
      name: 'skills.copyToProject.copy',
    }) as HTMLButtonElement).disabled).toBe(true);

    expect(mocks.selectGlobal).not.toHaveBeenCalled();
    expect(mocks.selectProject).not.toHaveBeenCalled();
    expect(mocks.switchEnvironment).not.toHaveBeenCalled();
    await waitFor(() => expect(mocks.execute).toHaveBeenCalledWith({
      kind: 'prepareCopyTarget',
      environment: ubuntu.environment,
    }));
  });
});
