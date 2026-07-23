/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen, waitFor } from '@testing-library/react';
import { SkillsPanel } from '../SkillsPanel';
import type { AppError, ContextRef, InstalledSkill, ProjectInfo } from '@/bindings';

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

function snapshot(
  skills: InstalledSkill[] = [],
  loading = false,
  error: AppError | null = null,
) {
  return {
    skills,
    agents: [],
    pathExists: true,
    loading,
    error,
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
    refreshWorkspace: vi.fn().mockResolvedValue(undefined),
    syncUpdates: vi.fn().mockResolvedValue(undefined),
    forceCheckUpdates: vi.fn().mockResolvedValue(true),
    syncSkills: vi.fn().mockResolvedValue(undefined),
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
  updateWorkflowState: { phase: 'closed', context: null as ContextRef | null, skillNames: [] as string[], open: vi.fn().mockResolvedValue(true) },
  updateWorkflowSelectors: [] as Array<(state: {
    phase: string;
    context: ContextRef | null;
    skillNames: string[];
    open: (...args: unknown[]) => unknown;
  }) => unknown>,
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

vi.mock('@/workflows/skill-update', () => ({
  useSkillUpdateWorkflow: (selector: (state: typeof mocks.updateWorkflowState) => unknown) => {
    mocks.updateWorkflowSelectors.push(selector as never);
    return selector(mocks.updateWorkflowState);
  },
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
    updatingSkills,
    onRepairSource,
    onCheckUpdates,
    onPrepareUpdate,
    scope,
  }: {
    skills: Array<{ name: string; scope: 'global' | 'project' }>;
    updatingSkills: Map<string, string>;
    onRepairSource?: (skill: { name: string; scope: 'global' | 'project' }) => void;
    onCheckUpdates?: () => Promise<boolean>;
    onPrepareUpdate: (skillNames: string[], batch: boolean) => Promise<boolean>;
    scope: 'global' | 'project';
  }) => (
    <div>
      skills-section
      {skills.map((skill) => (
        <div key={`${skill.scope}:${skill.name}`}>
          <span data-testid={`phase:${skill.scope}:${skill.name}`}>
            {updatingSkills.get(`${skill.scope}:${skill.name}`) ?? 'idle'}
          </span>
          <button
            type="button"
            data-testid={`repair:${skill.scope}:${skill.name}`}
            onClick={() => onRepairSource?.(skill)}
          >
            repair
          </button>
        </div>
      ))}
      <button type="button" data-testid={`check:${scope}`} onClick={() => void onCheckUpdates?.()}>
        check
      </button>
      <button
        type="button"
        data-testid={`update:${scope}`}
        onClick={() => void onPrepareUpdate([skills[0]?.name ?? 'missing'], false)}
      >
        update
      </button>
      <button
        type="button"
        data-testid={`update-all:${scope}`}
        onClick={() => void onPrepareUpdate(skills.map((skill) => skill.name), true)}
      >
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
    mocks.skillsDataState.refreshWorkspace.mockClear();
    mocks.skillsDataState.syncUpdates.mockClear();
    mocks.skillsDataState.forceCheckUpdates.mockClear();
    mocks.updateWorkflowState.open.mockClear();
    mocks.updateWorkflowState.phase = 'closed';
    mocks.updateWorkflowState.context = null;
    mocks.updateWorkflowState.skillNames = [];
    mocks.updateWorkflowSelectors.length = 0;
    mocks.skillsDataState.syncSkills.mockClear();
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

  it('projects an executing update into the matching list row', () => {
    mocks.skillsDataState.snapshots = {
      'host/global': snapshot([makeSkill('toolkit')]),
    };
    mocks.updateWorkflowState.phase = 'executing';
    mocks.updateWorkflowState.context = hostGlobal;
    mocks.updateWorkflowState.skillNames = ['toolkit'];

    render(<SkillsPanel compact={false} />);

    expect(screen.getByTestId('phase:global:toolkit').textContent).toBe('updating');
  });

  it('keeps update subscriptions stable while the preview dialog opens', () => {
    mocks.skillsDataState.snapshots = {
      'host/global': snapshot([makeSkill('toolkit')]),
    };

    render(<SkillsPanel compact={false} />);

    const closed = {
      ...mocks.updateWorkflowState,
      phase: 'closed',
      context: null,
      skillNames: [],
    };
    const loadingPreview = {
      ...closed,
      phase: 'loadingPreview',
      context: hostGlobal,
      skillNames: ['toolkit'],
    };
    const ready = { ...loadingPreview, phase: 'ready' };
    const executing = { ...loadingPreview, phase: 'executing' };

    const closedValues = mocks.updateWorkflowSelectors.map((selector) => selector(closed));
    expect(mocks.updateWorkflowSelectors.every((selector, index) => (
      Object.is(selector(loadingPreview), closedValues[index])
    ))).toBe(true);
    expect(mocks.updateWorkflowSelectors.every((selector, index) => (
      Object.is(selector(ready), closedValues[index])
    ))).toBe(true);
    expect(mocks.updateWorkflowSelectors.some((selector, index) => (
      !Object.is(selector(executing), closedValues[index])
    ))).toBe(true);
  });

  it('formats a structured load error and retries the committed context', async () => {
    mocks.workspaceContextState.selectedContext = ubuntuGlobal;
    mocks.skillsDataState.snapshots = {
      'wsl:ubuntu/global': snapshot([], false, {
        kind: 'custom',
        data: { message: 'invalid WSL inspect record' },
      }),
    };

    render(<SkillsPanel compact={false} />);

    await waitFor(() => {
      expect(mocks.skillsDataState.refreshWorkspace).toHaveBeenCalledWith(ubuntuGlobal);
    });
    mocks.skillsDataState.refreshWorkspace.mockClear();

    expect(screen.getByText('invalid WSL inspect record')).toBeDefined();
    screen.getByRole('button', { name: 'skills.retry' }).click();

    expect(mocks.skillsDataState.refreshWorkspace).toHaveBeenCalledWith(ubuntuGlobal);
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
      'wsl:ubuntu/global': snapshot(),
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
      'wsl:ubuntu': [{
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
      'wsl:ubuntu/global': snapshot([makeSkill('global-skill')]),
      'wsl:ubuntu/project:project-a': snapshot([makeSkill('project-skill', 'project')]),
    };

    render(<SkillsPanel compact={false} />);

    document.querySelector<HTMLButtonElement>('[data-testid="check:project"]')?.click();
    document.querySelector<HTMLButtonElement>('[data-testid="update:project"]')?.click();
    document.querySelector<HTMLButtonElement>('[data-testid="update-all:project"]')?.click();

    expect(mocks.skillsDataState.forceCheckUpdates).toHaveBeenCalledWith(ubuntuProject, { kind: 'all' });
    expect(mocks.updateWorkflowState.open).toHaveBeenNthCalledWith(
      1,
      ubuntuProject,
      ['project-skill'],
      false,
    );
    expect(mocks.updateWorkflowState.open).toHaveBeenNthCalledWith(
      2,
      ubuntuProject,
      ['project-skill'],
      true,
    );
  });

  it('debounces automatic checks and cancels unsent focus work on unmount', async () => {
    vi.useFakeTimers();
    const { unmount } = render(<SkillsPanel compact={false} />);
    await act(async () => { await Promise.resolve(); });

    await act(async () => { await vi.advanceTimersByTimeAsync(499); });
    expect(mocks.skillsDataState.syncUpdates).not.toHaveBeenCalled();
    await act(async () => { await vi.advanceTimersByTimeAsync(1); });
    expect(mocks.skillsDataState.syncUpdates).toHaveBeenCalledTimes(1);

    mocks.skillsDataState.syncUpdates.mockClear();
    window.dispatchEvent(new Event('focus'));
    window.dispatchEvent(new Event('focus'));
    await act(async () => { await vi.advanceTimersByTimeAsync(500); });
    expect(mocks.skillsDataState.syncUpdates).toHaveBeenCalledTimes(1);

    mocks.skillsDataState.syncUpdates.mockClear();
    window.dispatchEvent(new Event('focus'));
    unmount();
    await act(async () => { await vi.advanceTimersByTimeAsync(500); });
    expect(mocks.skillsDataState.syncUpdates).not.toHaveBeenCalled();
    vi.useRealTimers();
  });

  it('sends the automatic check at 500ms even while workspace refresh is unresolved', async () => {
    vi.useFakeTimers();
    mocks.skillsDataState.refreshWorkspace.mockImplementationOnce(() => new Promise<void>(() => undefined));
    render(<SkillsPanel compact={false} />);

    await act(async () => { await vi.advanceTimersByTimeAsync(500); });

    expect(mocks.skillsDataState.syncUpdates).toHaveBeenCalledWith(hostGlobal);
    vi.useRealTimers();
  });

  it('cancels a pending automatic check when the selected context changes', async () => {
    vi.useFakeTimers();
    const { rerender } = render(<SkillsPanel compact={false} />);
    await act(async () => { await Promise.resolve(); });

    mocks.workspaceContextState.selectedContext = ubuntuGlobal;
    rerender(<SkillsPanel compact={false} />);
    await act(async () => { await Promise.resolve(); });
    await act(async () => { await vi.advanceTimersByTimeAsync(500); });

    expect(mocks.skillsDataState.syncUpdates).toHaveBeenCalledTimes(1);
    expect(mocks.skillsDataState.syncUpdates).toHaveBeenCalledWith(ubuntuGlobal);
    vi.useRealTimers();
  });

  it('does not cancel a check that was already sent before a context switch', async () => {
    vi.useFakeTimers();
    let resolveSync: (() => void) | undefined;
    mocks.skillsDataState.syncUpdates.mockImplementationOnce(() => new Promise<void>((resolve) => {
      resolveSync = resolve;
    }));
    const { rerender } = render(<SkillsPanel compact={false} />);
    await act(async () => { await Promise.resolve(); });
    await act(async () => { await vi.advanceTimersByTimeAsync(500); });

    expect(mocks.skillsDataState.syncUpdates).toHaveBeenCalledWith(hostGlobal);
    mocks.workspaceContextState.selectedContext = ubuntuGlobal;
    rerender(<SkillsPanel compact={false} />);
    await act(async () => { await Promise.resolve(); });

    expect(mocks.skillsDataState.syncUpdates).toHaveBeenCalledTimes(1);
    resolveSync?.();
    vi.useRealTimers();
  });
});
