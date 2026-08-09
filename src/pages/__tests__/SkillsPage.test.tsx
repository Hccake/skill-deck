/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, waitFor } from '@testing-library/react';
import { useEffect } from 'react';
import { SkillsPage } from '../SkillsPage';
import type { SkillLocationRef, InstalledSkill, ProjectInfo } from '@/bindings';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

const mocks = vi.hoisted(() => ({
  workspaceContextState: {
    selectedContext: {
      environment: { kind: 'native' },
      scope: { scope: 'global' },
    } as SkillLocationRef,
  },
  projectState: {
    projectsByEnvironment: {} as Record<string, ProjectInfo[]>,
    refresh: vi.fn().mockResolvedValue(undefined),
  },
  environmentState: {
    environments: [
      { environment: { kind: 'native' as const }, displayName: 'Native', status: 'available' as const },
      { environment: { kind: 'wsl' as const, distro_name: 'Ubuntu' }, displayName: 'Ubuntu', status: 'available' as const },
    ],
    connect: vi.fn().mockResolvedValue(undefined),
  },
  tauriApi: {
    listSkills: vi.fn(),
  },
  skillsDataState: {
    snapshots: {} as Record<string, {
      skills: InstalledSkill[];
      agents: Array<{ id: string; name: string }>;
      pathExists: boolean;
      loading: boolean;
      error: string | null;
      requestId: number;
    }>,
    checkingUpdateScopes: new Set<string>(),
    forceCheckUpdates: vi.fn(),
  },
  skillDetailState: {
    selectedSkillRef: null as null | { name: string; scope: 'global' | 'project'; projectPath?: string | null },
    skillContent: null as string | null,
    loadingContent: false,
    deselectSkill: vi.fn(),
    reloadContent: vi.fn(),
  },
  skillDialogState: {
    openDelete: vi.fn(),
    openManageAgents: vi.fn(),
    closeManageAgents: vi.fn(),
    saveAgentChanges: vi.fn(),
    manageAgentsSkill: null,
    copySkill: null as InstalledSkill | null,
    copyContext: null as SkillLocationRef | null,
    repairSourceTarget: null,
    openCopyToProject: vi.fn(),
    closeCopyToProject: vi.fn(),
    executeCopy: vi.fn(),
  },
  resizable: {
    groups: [] as Array<Record<string, unknown>>,
    panels: [] as Array<Record<string, unknown>>,
    lifecycle: [] as string[],
    setLayout: vi.fn(),
    getLayout: vi.fn(() => ({})),
  },
  skillsPanelLifecycle: [] as string[],
  updateWorkflowState: {
    phase: 'closed', context: null as SkillLocationRef | null, skillNames: [] as string[],
    open: vi.fn().mockResolvedValue(true), close: vi.fn(),
  },
}));

const nativeGlobal: SkillLocationRef = {
  environment: { kind: 'native' },
  scope: { scope: 'global' },
};

function snapshot(skills: InstalledSkill[] = []) {
  return {
    skills,
    agents: [],
    pathExists: true,
    loading: false,
    error: null,
    requestId: 1,
  };
}

function makeSkill(name: string, overrides: Partial<InstalledSkill> = {}): InstalledSkill {
  return {
    name,
    description: `${name} description`,
    path: `/skills/${name}`,
    canonicalPath: `/canonical/${name}`,
    scope: 'global',
    agents: [],
    associatedAgents: [],
    hasUpdate: false,
    ...overrides,
  };
}

vi.mock('@/stores/workspace-context', () => ({
  useWorkspaceContextStore: (selector: (state: typeof mocks.workspaceContextState) => unknown) =>
    selector(mocks.workspaceContextState),
}));

vi.mock('@/hooks/useProjectWorkspace', () => ({
  useProjectWorkspace: (environment: { kind: string; distro_name?: string }) => {
    const key = environment.kind === 'native' ? 'native' : `wsl:${environment.distro_name?.toLowerCase()}`;
    return {
      projects: mocks.projectState.projectsByEnvironment[key] ?? [],
      hasCompleteSnapshot: true,
      error: null,
      status: 'available',
      refresh: mocks.projectState.refresh,
      add: vi.fn(),
      remove: vi.fn(),
      setCrossStorageWarning: vi.fn(),
    };
  },
  useProjectCatalog: () => mocks.projectState.projectsByEnvironment,
}));

vi.mock('@/stores/projects', () => ({
  projectWorkspace: {
    execute: vi.fn().mockResolvedValue({ status: 'succeeded' }),
    getSnapshot: (environment: { kind: string; distro_name?: string }) => {
      const key = environment.kind === 'native' ? 'native' : `wsl:${environment.distro_name?.toLowerCase()}`;
      return { projects: mocks.projectState.projectsByEnvironment[key] ?? [] };
    },
  },
}));

vi.mock('@/stores/environment', () => ({
  useEnvironmentStore: Object.assign(
    (selector: (state: typeof mocks.environmentState) => unknown) => selector(mocks.environmentState),
    { getState: () => mocks.environmentState },
  ),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  listSkills: mocks.tauriApi.listSkills,
}));

vi.mock('@/stores/skills-data', () => ({
  sourceDiagnosticsForEnvironment: (snapshots: typeof mocks.skillsDataState.snapshots) => (
    Object.values(snapshots).flatMap((item) => (
      (item as typeof item & { updateCheck?: { sources: unknown[] } }).updateCheck?.sources ?? []
    ))
  ),
  useSkillsDataStore: (selector: (state: typeof mocks.skillsDataState) => unknown) => selector(mocks.skillsDataState),
}));

vi.mock('@/stores/skill-detail', () => ({
  useSkillDetailStore: (selector: (state: typeof mocks.skillDetailState) => unknown) => selector(mocks.skillDetailState),
}));

vi.mock('@/stores/skill-dialog', () => ({
  useSkillDialogStore: (selector: (state: typeof mocks.skillDialogState) => unknown) => selector(mocks.skillDialogState),
}));

vi.mock('@/workflows/skill-update', () => ({
  useSkillUpdateWorkflow: (selector: (state: typeof mocks.updateWorkflowState) => unknown) => selector(mocks.updateWorkflowState),
}));

vi.mock('@/components/skills', () => ({
  ContextSidebar: () => <div>context-sidebar</div>,
  SkillsPanel: ({ compact }: { compact?: boolean }) => {
    useEffect(() => {
      mocks.skillsPanelLifecycle.push('mount');
      return () => {
        mocks.skillsPanelLifecycle.push('unmount');
      };
    }, []);
    return <div data-compact={compact ? 'true' : 'false'}>skills-panel</div>;
  },
  SkillDetailPanel: ({
    skill,
    updateStatus,
    isCheckingUpdates,
    onCheckUpdates,
    onUpdate,
    onManageAgents,
  }: {
    skill: InstalledSkill;
    updateStatus?: string;
    isCheckingUpdates?: boolean;
    onCheckUpdates?: () => void;
    onUpdate?: (name: string, scope: 'global' | 'project') => void;
    onManageAgents?: (skill: InstalledSkill) => void;
  }) => (
    <div>
      <button
        type="button"
        data-skill-name={skill.name}
        data-update-status={updateStatus ?? 'idle'}
        data-checking-updates={isCheckingUpdates ? 'true' : 'false'}
        onClick={onCheckUpdates}
      >
        skill-detail-panel
      </button>
      <button type="button" onClick={() => onUpdate?.(skill.name, skill.scope)}>
        detail-update
      </button>
      <button type="button" onClick={() => onManageAgents?.(skill)}>
        detail-manage-agents
      </button>
    </div>
  ),
}));

vi.mock('@/components/skills/UpdatePlanDialogContainer', () => ({
  UpdatePlanDialogContainer: () => (
    <div
      data-testid="page-update-dialog"
      data-open={mocks.updateWorkflowState.phase !== 'closed' ? 'true' : 'false'}
    />
  ),
}));

vi.mock('@/components/ui/resizable', () => ({
  ResizablePanelGroup: ({ children, ...props }: React.PropsWithChildren<Record<string, unknown>>) => {
    mocks.resizable.groups.push(props);
    const groupRef = props.groupRef as { current?: { setLayout: typeof mocks.resizable.setLayout; getLayout: typeof mocks.resizable.getLayout } } | undefined;
    if (groupRef) {
      groupRef.current = {
        setLayout: mocks.resizable.setLayout,
        getLayout: mocks.resizable.getLayout,
      };
    }
    useEffect(() => {
      const id = String(props.id ?? 'group');
      mocks.resizable.lifecycle.push(`mount:${id}`);
      return () => {
        mocks.resizable.lifecycle.push(`unmount:${id}`);
      };
    }, [props.id]);
    return <div>{children}</div>;
  },
  ResizablePanel: ({ children, ...props }: React.PropsWithChildren<Record<string, unknown>>) => {
    mocks.resizable.panels.push(props);
    return <div>{children}</div>;
  },
  ResizableHandle: () => <div>handle</div>,
}));

describe('SkillsPage', () => {
  beforeEach(() => {
    mocks.workspaceContextState.selectedContext = nativeGlobal;
    mocks.projectState.projectsByEnvironment = {};
    mocks.projectState.refresh.mockReset();
    mocks.projectState.refresh.mockResolvedValue(undefined);
    mocks.environmentState.connect.mockReset();
    mocks.environmentState.connect.mockResolvedValue(undefined);
    mocks.tauriApi.listSkills.mockReset();
    mocks.tauriApi.listSkills.mockResolvedValue({ skills: [], agents: [], pathExists: true });
    mocks.skillsDataState.snapshots = { 'native/global': snapshot() };
    mocks.skillsDataState.checkingUpdateScopes = new Set();
    mocks.skillsDataState.forceCheckUpdates.mockReset();
    mocks.updateWorkflowState.phase = 'closed';
    mocks.updateWorkflowState.context = null;
    mocks.updateWorkflowState.skillNames = [];
    mocks.updateWorkflowState.open.mockReset();
    mocks.updateWorkflowState.open.mockResolvedValue(true);
    mocks.updateWorkflowState.close.mockReset();
    mocks.skillDetailState.selectedSkillRef = null;
    mocks.skillDetailState.skillContent = null;
    mocks.skillDetailState.loadingContent = false;
    mocks.skillDetailState.deselectSkill.mockReset();
    mocks.skillDetailState.reloadContent.mockReset();
    mocks.skillDialogState.openDelete.mockReset();
    mocks.skillDialogState.openManageAgents.mockReset();
    mocks.skillDialogState.copySkill = null;
    mocks.skillDialogState.copyContext = null;
    mocks.resizable.groups.length = 0;
    mocks.resizable.panels.length = 0;
    mocks.resizable.lifecycle.length = 0;
    mocks.resizable.setLayout.mockReset();
    mocks.resizable.getLayout.mockReset();
    mocks.resizable.getLayout.mockReturnValue({});
    mocks.skillsPanelLifecycle.length = 0;
  });

  it('uses percentage-based panel sizes when a skill detail is open', () => {
    mocks.skillDetailState.selectedSkillRef = {
      name: 'test-skill',
      scope: 'global',
    };
    mocks.skillsDataState.snapshots['native/global'] = snapshot([makeSkill('test-skill')]);

    render(<SkillsPage />);

    expect(mocks.resizable.groups[0]).toMatchObject({
      id: 'skills-page-layout',
      orientation: 'horizontal',
    });
    expect(mocks.resizable.panels[0]).toMatchObject({
      id: 'skills-list-panel',
      defaultSize: '22%',
      minSize: '12%',
      maxSize: '85%',
    });
    expect(mocks.resizable.panels[1]).toMatchObject({
      id: 'skill-detail-panel',
      defaultSize: '78%',
      minSize: '15%',
    });
  });

  it('renders the skills page container shell for responsive sidebar sizing', () => {
    const { container } = render(<SkillsPage />);

    expect(container.querySelector('.skills-page-shell')).toBeTruthy();
  });

  it('updates the panel layout without remounting the group when entering split view', () => {
    const { rerender } = render(<SkillsPage />);

    expect(mocks.resizable.lifecycle).toEqual(['mount:skills-page-layout']);
    expect(mocks.skillsPanelLifecycle).toEqual(['mount']);

    mocks.resizable.getLayout.mockReturnValue({
      'skills-list-panel': 22,
      'skill-detail-panel': 78,
    });

    mocks.skillDetailState.selectedSkillRef = {
      name: 'test-skill',
      scope: 'global',
    };
    mocks.skillsDataState.snapshots['native/global'] = snapshot([makeSkill('test-skill')]);

    rerender(<SkillsPage />);

    expect(mocks.resizable.lifecycle).toEqual(['mount:skills-page-layout']);
    expect(mocks.skillsPanelLifecycle).toEqual(['mount']);
    expect(mocks.resizable.setLayout).toHaveBeenCalledWith({
      'skills-list-panel': 22,
      'skill-detail-panel': 78,
    });
  });

  it('waits for the target panel count before applying the split layout', async () => {
    const { rerender } = render(<SkillsPage />);

    mocks.resizable.getLayout
      .mockReturnValueOnce({ 'skills-list-panel': 100 })
      .mockReturnValueOnce({
        'skills-list-panel': 22,
        'skill-detail-panel': 78,
      });

    mocks.skillDetailState.selectedSkillRef = {
      name: 'test-skill',
      scope: 'global',
    };
    mocks.skillsDataState.snapshots['native/global'] = snapshot([makeSkill('test-skill')]);

    rerender(<SkillsPage />);

    expect(mocks.resizable.setLayout).not.toHaveBeenCalled();

    await waitFor(() => {
      expect(mocks.resizable.setLayout).toHaveBeenCalledWith({
        'skills-list-panel': 22,
        'skill-detail-panel': 78,
      });
    });
  });

  it('passes identity-keyed update status to the detail panel', () => {
    mocks.skillDetailState.selectedSkillRef = {
      name: 'toolkit',
      scope: 'global',
    };
    mocks.skillsDataState.snapshots['native/global'] = snapshot([makeSkill('toolkit')]);
    mocks.updateWorkflowState.phase = 'executing';
    mocks.updateWorkflowState.context = nativeGlobal;
    mocks.updateWorkflowState.skillNames = ['toolkit'];

    const { getByText } = render(<SkillsPage />);

    expect(getByText('skill-detail-panel').getAttribute('data-update-status')).toBe('updating');
  });

  it('wires detail check-updates to the selected skill scope', () => {
    mocks.skillDetailState.selectedSkillRef = {
      name: 'toolkit',
      scope: 'global',
    };
    mocks.skillsDataState.snapshots['native/global'] = snapshot([makeSkill('toolkit')]);
    mocks.skillsDataState.checkingUpdateScopes = new Set(['native/global']);

    const { getByText } = render(<SkillsPage />);
    const detailButton = getByText('skill-detail-panel');

    expect(detailButton.getAttribute('data-checking-updates')).toBe('true');

    fireEvent.click(detailButton);

    expect(mocks.skillsDataState.forceCheckUpdates).toHaveBeenCalledWith(nativeGlobal, {
      kind: 'skills', skills: [{ context: nativeGlobal, skillName: 'toolkit' }],
    });
  });

  it('routes detail updates through the shared preview dialog owner', () => {
    mocks.skillDetailState.selectedSkillRef = {
      name: 'toolkit',
      scope: 'global',
    };
    mocks.skillsDataState.snapshots['native/global'] = snapshot([
      makeSkill('toolkit', { hasUpdate: true }),
    ]);
    mocks.updateWorkflowState.phase = 'ready';
    mocks.updateWorkflowState.context = nativeGlobal;
    mocks.updateWorkflowState.skillNames = ['toolkit'];

    const { getByText, getByTestId } = render(<SkillsPage />);
    fireEvent.click(getByText('detail-update'));

    expect(mocks.updateWorkflowState.open).toHaveBeenCalledWith(
      nativeGlobal,
      ['toolkit'],
      false,
    );
    expect(getByTestId('page-update-dialog').getAttribute('data-open')).toBe('true');
  });

  it('opens detail Agent management with the selected operation context', () => {
    mocks.skillDetailState.selectedSkillRef = {
      name: 'toolkit',
      scope: 'global',
    };
    mocks.skillsDataState.snapshots['native/global'] = snapshot([makeSkill('toolkit')]);

    const { getByText } = render(<SkillsPage />);
    fireEvent.click(getByText('detail-manage-agents'));

    expect(mocks.skillDialogState.openManageAgents).toHaveBeenCalledWith(
      expect.objectContaining({ name: 'toolkit', scope: 'global' }),
      nativeGlobal,
    );
  });

  it('uses the explicit environment key for detail update progress', () => {
    const context: SkillLocationRef = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    };
    mocks.workspaceContextState.selectedContext = context;
    mocks.skillDetailState.selectedSkillRef = {
      name: 'toolkit',
      scope: 'global',
    };
    mocks.skillsDataState.snapshots = {
      'wsl:ubuntu/global': snapshot([makeSkill('toolkit')]),
    };
    mocks.skillsDataState.checkingUpdateScopes = new Set(['wsl:ubuntu/global']);

    const { getByText } = render(<SkillsPage />);

    expect(getByText('skill-detail-panel').getAttribute('data-checking-updates')).toBe('true');
  });

  it('shows copy targets from the selected WSL environment', async () => {
    mocks.workspaceContextState.selectedContext = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'current' },
    };
    mocks.projectState.projectsByEnvironment = {
      'wsl:ubuntu': [
        {
          binding: {
            id: 'current',
            nativePath: '/home/me/current',
            displayName: null,
            order: null,
            suppressCrossStorageWarning: false,
          },
          storage: {
            access: 'native',
            owner: { kind: 'wsl', distro_name: 'Ubuntu' },
          },
        },
        {
          binding: {
            id: 'target',
            nativePath: '/home/me/target',
            displayName: null,
            order: null,
            suppressCrossStorageWarning: false,
          },
          storage: {
            access: 'native',
            owner: { kind: 'wsl', distro_name: 'Ubuntu' },
          },
        },
      ],
    };
    mocks.skillsDataState.snapshots = {
      'wsl:ubuntu/global': snapshot(),
      'wsl:ubuntu/project:current': snapshot(),
    };
    mocks.skillDialogState.copySkill = makeSkill('toolkit', { scope: 'project' });
    mocks.skillDialogState.copyContext = mocks.workspaceContextState.selectedContext;

    const { getByText, queryByText } = render(<SkillsPage />);

    await waitFor(() => expect(getByText('/home/me/target')).toBeDefined());
    expect(queryByText('/home/me/current')).toBeNull();
  });

  it('keeps failed copy target inspection unknown instead of treating it as absent', async () => {
    mocks.workspaceContextState.selectedContext = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'current' },
    };
    mocks.projectState.projectsByEnvironment = {
      'wsl:ubuntu': [
        {
          binding: {
            id: 'current',
            nativePath: '/home/me/current',
            displayName: null,
            order: null,
            suppressCrossStorageWarning: false,
          },
          storage: {
            access: 'native',
            owner: { kind: 'wsl', distro_name: 'Ubuntu' },
          },
        },
        {
          binding: {
            id: 'target',
            nativePath: '/home/me/target',
            displayName: null,
            order: null,
            suppressCrossStorageWarning: false,
          },
          storage: {
            access: 'native',
            owner: { kind: 'wsl', distro_name: 'Ubuntu' },
          },
        },
      ],
    };
    mocks.skillsDataState.snapshots = {
      'wsl:ubuntu/global': snapshot(),
      'wsl:ubuntu/project:current': snapshot(),
    };
    mocks.skillDialogState.copySkill = makeSkill('toolkit', { scope: 'project' });
    mocks.skillDialogState.copyContext = mocks.workspaceContextState.selectedContext;
    mocks.tauriApi.listSkills.mockRejectedValue(new Error('inspection failed'));

    const { findByRole, getByText } = render(<SkillsPage />);

    expect(await findByRole('status', {
      name: 'skills.copyToProject.presenceUnknown',
    })).toBeDefined();
    expect(getByText('/home/me/target').closest('label')?.textContent).toContain(
      'skills.copyToProject.unknown',
    );
  });

  it('derives the selected skill from the shared skills store', () => {
    mocks.skillDetailState.selectedSkillRef = {
      name: 'toolkit',
      scope: 'global',
    };
    mocks.skillsDataState.snapshots['native/global'] = snapshot([
      makeSkill('toolkit', { updatedAt: '2026-04-07T12:00:00.000Z' }),
    ]);

    const { getByText } = render(<SkillsPage />);

    expect(getByText('skill-detail-panel').getAttribute('data-skill-name')).toBe('toolkit');
  });

  it('deselects a stale skill identity when it no longer resolves in the current context', async () => {
    mocks.skillDetailState.selectedSkillRef = {
      name: 'missing-skill',
      scope: 'global',
    };
    mocks.skillsDataState.snapshots['native/global'] = snapshot();

    render(<SkillsPage />);

    await waitFor(() => {
      expect(mocks.skillDetailState.deselectSkill).toHaveBeenCalledTimes(1);
    });
  });
});
