/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { SkillsPanel } from '../SkillsPanel';
import type { ContextRef, InstalledSkill, ProjectInfo } from '@/bindings';

const hostGlobal: ContextRef = {
  environment: { kind: 'host' },
  scope: { scope: 'global' },
};
const ubuntuGlobal: ContextRef = {
  environment: { kind: 'wsl', distro_name: 'Ubuntu' },
  scope: { scope: 'global' },
};
const ubuntuProject: ContextRef = {
  environment: ubuntuGlobal.environment,
  scope: { scope: 'project', project_id: 'project-a' },
};

function makeSkill(name: string, scope: 'global' | 'project' = 'global'): InstalledSkill {
  return {
    name,
    description: '',
    path: `/skills/${name}`,
    canonicalPath: `/canonical/${name}`,
    scope,
    agents: [],
    hasUpdate: false,
    canRunUpdate: false,
    canCheckForUpdates: false,
    updateReason: 'missing-skill-path',
    source: 'owner/repo',
    sourceUrl: 'https://github.com/owner/repo',
  };
}

function snapshot(skills: InstalledSkill[] = [], loading = false) {
  return {
    skills,
    agents: [],
    pathExists: true,
    loading,
    error: null,
    requestId: 1,
  };
}

const mocks = vi.hoisted(() => ({
  workspaceContextState: {
    selectedContext: {
      environment: { kind: 'host' as const },
      scope: { scope: 'global' as const },
    } as ContextRef,
  },
  projectState: {
    projectsByEnvironment: {} as Record<string, ProjectInfo[]>,
  },
  skillsDataState: {
    snapshots: {} as Record<string, ReturnType<typeof snapshot>>,
    isSyncing: false,
    checkingUpdateScopes: new Set<string>(),
    updatingSkills: new Map<string, 'updating' | 'done' | 'failed'>(),
    refreshWorkspace: vi.fn().mockResolvedValue(undefined),
    syncUpdates: vi.fn().mockResolvedValue(undefined),
    forceCheckUpdates: vi.fn().mockResolvedValue(true),
    updateAllInSection: vi.fn().mockResolvedValue(undefined),
    syncSkills: vi.fn().mockResolvedValue(undefined),
    updateSkill: vi.fn().mockResolvedValue(undefined),
    auditCache: {},
    fetchAuditForSkills: vi.fn<() => Promise<void>>().mockResolvedValue(undefined),
  },
  skillDetailState: {
    selectSkill: vi.fn<() => Promise<void>>().mockResolvedValue(undefined),
    deselectSkill: vi.fn(),
    selectedSkillRef: {
      name: 'brainstorming',
      scope: 'global' as const,
    },
  },
  skillDialogState: {
    openDelete: vi.fn(),
    openAdd: vi.fn(),
    openRepairSource: vi.fn(),
  },
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock('@/stores/workspace-context', () => ({
  useWorkspaceContextStore: (selector: (state: typeof mocks.workspaceContextState) => unknown) =>
    selector(mocks.workspaceContextState),
}));

vi.mock('@/stores/projects', () => ({
  useProjectStore: (selector: (state: typeof mocks.projectState) => unknown) =>
    selector(mocks.projectState),
}));

vi.mock('@/stores/skills-data', () => ({
  useSkillsDataStore: (selector?: (state: typeof mocks.skillsDataState) => unknown) =>
    selector ? selector(mocks.skillsDataState) : mocks.skillsDataState,
}));

vi.mock('@/stores/skill-detail', () => ({
  useSkillDetailStore: (selector?: (state: typeof mocks.skillDetailState) => unknown) =>
    selector ? selector(mocks.skillDetailState) : mocks.skillDetailState,
}));

vi.mock('@/stores/skill-dialog', () => ({
  useSkillDialogStore: (selector?: (state: typeof mocks.skillDialogState) => unknown) =>
    selector ? selector(mocks.skillDialogState) : mocks.skillDialogState,
}));

vi.mock('../SkillsToolbar', () => ({
  SkillsToolbar: () => <div>skills-toolbar</div>,
}));

vi.mock('../CompactSkillList', () => ({
  CompactSkillList: () => <div>compact-skill-list</div>,
}));

vi.mock('../CrossStorageWarningBanner', () => ({
  CrossStorageWarningBanner: () => <div>cross-storage-warning</div>,
}));

vi.mock('../SkillsSection', () => ({
  SkillsSection: ({
    skills,
    onRepairSource,
    onCheckUpdates,
    onUpdate,
    onUpdateAll,
    scope,
  }: {
    skills: Array<{ name: string; scope: 'global' | 'project' }>;
    onRepairSource?: (skill: { name: string; scope: 'global' | 'project' }) => void;
    onCheckUpdates?: () => Promise<boolean>;
    onUpdate: (name: string, scope: 'global' | 'project') => Promise<void>;
    onUpdateAll: (scope: 'global' | 'project') => Promise<void>;
    scope: 'global' | 'project';
  }) => (
    <div>
      skills-section
      {skills.map((skill) => (
        <button
          key={`${skill.scope}:${skill.name}`}
          type="button"
          data-testid={`repair:${skill.scope}:${skill.name}`}
          onClick={() => onRepairSource?.(skill)}
        >
          repair
        </button>
      ))}
      <button type="button" data-testid={`check:${scope}`} onClick={() => void onCheckUpdates?.()}>
        check
      </button>
      <button
        type="button"
        data-testid={`update:${scope}`}
        onClick={() => void onUpdate(skills[0]?.name ?? 'missing', scope)}
      >
        update
      </button>
      <button type="button" data-testid={`update-all:${scope}`} onClick={() => void onUpdateAll(scope)}>
        update all
      </button>
    </div>
  ),
}));

vi.mock('../DeleteSkillDialog', () => ({
  DeleteSkillDialog: () => <div>delete-skill-dialog</div>,
}));

vi.mock('../EmptyStates', () => ({
  GlobalEmptyState: () => <div>global-empty-state</div>,
  ProjectEmptyState: () => <div>project-empty-state</div>,
}));

describe('SkillsPanel', () => {
  beforeEach(() => {
    mocks.workspaceContextState.selectedContext = hostGlobal;
    mocks.projectState.projectsByEnvironment = {};
    mocks.skillsDataState.snapshots = {
      'host/global': snapshot(),
    };
    mocks.skillsDataState.isSyncing = false;
    mocks.skillsDataState.checkingUpdateScopes = new Set();
    mocks.skillsDataState.updatingSkills = new Map();
    mocks.skillsDataState.refreshWorkspace.mockClear();
    mocks.skillsDataState.syncUpdates.mockClear();
    mocks.skillsDataState.forceCheckUpdates.mockClear();
    mocks.skillsDataState.updateAllInSection.mockClear();
    mocks.skillsDataState.syncSkills.mockClear();
    mocks.skillsDataState.updateSkill.mockClear();
    mocks.skillsDataState.auditCache = {};
    mocks.skillsDataState.fetchAuditForSkills.mockClear();
    mocks.skillDetailState.selectSkill.mockClear();
    mocks.skillDetailState.deselectSkill.mockClear();
    mocks.skillDetailState.selectedSkillRef = {
      name: 'brainstorming',
      scope: 'global',
    };
    mocks.skillDialogState.openDelete.mockClear();
    mocks.skillDialogState.openAdd.mockClear();
    mocks.skillDialogState.openRepairSource.mockClear();
  });

  it('does not clear the selected skill when compact mode mounts', async () => {
    render(<SkillsPanel compact />);

    await waitFor(() => {
      expect(mocks.skillsDataState.refreshWorkspace).toHaveBeenCalledWith(hostGlobal);
    });

    expect(mocks.skillDetailState.deselectSkill).not.toHaveBeenCalled();
    expect(screen.getByText('cross-storage-warning')).toBeDefined();
  });

  it('opens the repair source dialog for repairable skills instead of the install wizard', async () => {
    mocks.skillsDataState.snapshots = {
      'host/global': snapshot([makeSkill('toolkit')]),
    };

    render(<SkillsPanel compact={false} />);

    await waitFor(() => {
      expect(mocks.skillsDataState.refreshWorkspace).toHaveBeenCalledWith(hostGlobal);
    });

    document.querySelector<HTMLButtonElement>('[data-testid="repair:global:toolkit"]')?.click();

    expect(mocks.skillDialogState.openRepairSource).toHaveBeenCalledWith(
      expect.objectContaining({ name: 'toolkit', scope: 'global' }),
      hostGlobal,
    );
    expect(mocks.skillDialogState.openAdd).not.toHaveBeenCalled();
  });

  it('keeps cached rows visible while the committed context refreshes', () => {
    mocks.skillsDataState.snapshots = {
      'host/global': snapshot([makeSkill('cached')], true),
    };

    render(<SkillsPanel compact={false} />);

    expect(screen.getByTestId('repair:global:cached')).toBeDefined();
  });

  it('refreshes and clears details when the committed context changes', async () => {
    const { rerender } = render(<SkillsPanel compact />);

    await waitFor(() => {
      expect(mocks.skillsDataState.refreshWorkspace).toHaveBeenCalledWith(hostGlobal);
    });

    mocks.skillDetailState.deselectSkill.mockClear();
    mocks.skillsDataState.refreshWorkspace.mockClear();
    mocks.workspaceContextState.selectedContext = ubuntuGlobal;
    mocks.skillsDataState.snapshots = {
      ...mocks.skillsDataState.snapshots,
      'wsl:Ubuntu/global': snapshot(),
    };

    rerender(<SkillsPanel compact />);

    await waitFor(() => {
      expect(mocks.skillsDataState.refreshWorkspace).toHaveBeenCalledWith(ubuntuGlobal);
    });

    expect(mocks.skillDetailState.deselectSkill).toHaveBeenCalledTimes(1);
  });

  it('passes the exact section context to check and update actions', () => {
    mocks.workspaceContextState.selectedContext = ubuntuProject;
    mocks.projectState.projectsByEnvironment = {
      'wsl:Ubuntu': [{
        binding: {
          id: 'project-a',
          nativePath: '/home/me/project-a',
          displayName: null,
          order: null,
          suppressCrossStorageWarning: false,
        },
        storage: { access: 'native', owner: ubuntuProject.environment },
      }],
    };
    mocks.skillsDataState.snapshots = {
      'wsl:Ubuntu/global': snapshot([makeSkill('global-skill')]),
      'wsl:Ubuntu/project:project-a': snapshot([makeSkill('project-skill', 'project')]),
    };

    render(<SkillsPanel compact={false} />);

    document.querySelector<HTMLButtonElement>('[data-testid="check:project"]')?.click();
    document.querySelector<HTMLButtonElement>('[data-testid="update:project"]')?.click();
    document.querySelector<HTMLButtonElement>('[data-testid="update-all:project"]')?.click();

    expect(mocks.skillsDataState.forceCheckUpdates).toHaveBeenCalledWith(ubuntuProject);
    expect(mocks.skillsDataState.updateSkill).toHaveBeenCalledWith(ubuntuProject, 'project-skill');
    expect(mocks.skillsDataState.updateAllInSection).toHaveBeenCalledWith(ubuntuProject);
  });
});
