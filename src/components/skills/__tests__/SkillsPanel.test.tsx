/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { SkillsPanel } from '../SkillsPanel';
import { makeResolvedAgent } from '@/test-utils';
import type {
  AgentId,
  AppError,
  ContextRef,
  InstalledSkill,
  ProjectInfo,
  ResolvedAgent,
} from '@/bindings';
import type { ReactNode } from 'react';

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
    associatedAgents: [],
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
): {
  skills: InstalledSkill[];
  agents: ResolvedAgent[];
  pathExists: boolean;
  loading: boolean;
  error: AppError | null;
  requestId: number;
} {
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
    automaticUpdateScopes: new Set<string>(),
    forceUpdateScopes: new Set<string>(),
    refreshWorkspace: vi.fn().mockResolvedValue(undefined),
    syncUpdates: vi.fn().mockResolvedValue(undefined),
    activateAutomaticChecks: vi.fn().mockResolvedValue(undefined),
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

vi.mock('@/hooks/useProjectWorkspace', () => ({
  useProjectWorkspace: (environment: { kind: string; distro_name?: string }) => {
    const key = environment.kind === 'host' ? 'host' : `wsl:${environment.distro_name?.toLowerCase()}`;
    return { projects: mocks.projectState.projectsByEnvironment[key] ?? [] };
  },
}));

vi.mock('@/stores/skills-data', () => ({
  sourceDiagnosticsForEnvironment: (snapshots: typeof mocks.skillsDataState.snapshots) => (
    Object.values(snapshots).flatMap((item) => (
      (item as typeof item & { updateCheck?: { sources: unknown[] } }).updateCheck?.sources ?? []
    ))
  ),
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
  SkillsToolbar: ({
    compact,
    searchQuery,
    onSearchChange,
    selectedAgent,
    onAgentChange,
    filterableAgents,
    onSync,
  }: {
    compact?: boolean;
    searchQuery: string;
    onSearchChange: (query: string) => void;
    selectedAgent: AgentId | null;
    onAgentChange: (agentId: AgentId | null) => void;
    filterableAgents: ResolvedAgent[];
    onSync: () => void;
  }) => (
    <div>
      <span data-testid="toolbar-mode">{compact ? 'compact' : 'full'}</span>
      <span data-testid="search-query">{searchQuery}</span>
      <span data-testid="selected-agent">{selectedAgent ?? 'no-agent'}</span>
      <span data-testid="filterable-agents">
        {filterableAgents.map((agent) => agent.definition.id).join(',')}
      </span>
      {filterableAgents.map((agent) => (
        <button
          key={agent.definition.id}
          type="button"
          data-testid={`filter-agent:${agent.definition.id}`}
          onClick={() => onAgentChange(agent.definition.id)}
        >
          {agent.definition.displayName}
        </button>
      ))}
      <button type="button" data-testid="clear-agent-filter" onClick={() => onAgentChange(null)}>
        clear
      </button>
      <button type="button" data-testid="set-missing-search" onClick={() => onSearchChange('missing')}>
        search missing
      </button>
      <button type="button" data-testid="toolbar-sync" onClick={onSync}>
        sync
      </button>
    </div>
  ),
}));

vi.mock('../CompactSkillList', () => ({
  CompactSkillList: ({
    globalSkills,
    projectSkills,
    globalEmptyState,
    projectEmptyState,
  }: {
    globalSkills: InstalledSkill[];
    projectSkills: InstalledSkill[];
    globalEmptyState?: ReactNode;
    projectEmptyState?: ReactNode;
  }) => (
    <div>
      compact-skill-list
      <span data-testid="compact-skills">
        {[...globalSkills, ...projectSkills].map((skill) => skill.name).join(',')}
      </span>
      {projectSkills.length === 0 ? projectEmptyState : null}
      {globalSkills.length === 0 ? globalEmptyState : null}
    </div>
  ),
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
    emptyState,
  }: {
    skills: Array<{ name: string; scope: 'global' | 'project' }>;
    updatingSkills: Map<string, string>;
    onRepairSource?: (skill: { name: string; scope: 'global' | 'project' }) => void;
    onCheckUpdates?: () => Promise<boolean>;
    onPrepareUpdate: (skillNames: string[], batch: boolean) => Promise<boolean>;
    scope: 'global' | 'project';
    emptyState?: ReactNode;
  }) => (
    <div data-testid={`skills-section:${scope}`}>
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
      {skills.length === 0 ? emptyState : null}
    </div>
  ),
}));

vi.mock('../DeleteSkillDialog', () => ({
  DeleteSkillDialog: () => <div>delete-skill-dialog</div>,
}));

vi.mock('../EmptyStates', () => ({
  GlobalEmptyState: () => <div>global-empty-state</div>,
  ProjectEmptyState: () => <div>project-empty-state</div>,
  SkillFilterEmptyState: () => <div>skill-filter-empty-state</div>,
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
    mocks.skillsDataState.activateAutomaticChecks.mockClear();
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
    await waitFor(() => {
      expect(mocks.skillsDataState.activateAutomaticChecks).toHaveBeenCalledWith(ubuntuGlobal);
    });
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

  it('resets the main list scroll only when the selected Context changes', () => {
    mocks.workspaceContextState.selectedContext = ubuntuProject;
    mocks.skillsDataState.snapshots = {
      'wsl:ubuntu/global': snapshot([makeSkill('global-skill')]),
      'wsl:ubuntu/project:project-a': snapshot([makeSkill('project-skill', 'project')]),
    };

    const { container, rerender } = render(<SkillsPanel compact={false} />);
    const listScroll = container.querySelector<HTMLDivElement>('.flex-1.overflow-auto');
    expect(listScroll).not.toBeNull();
    listScroll!.scrollTop = 320;

    mocks.skillsDataState.snapshots = {
      ...mocks.skillsDataState.snapshots,
      'wsl:ubuntu/project:project-a': snapshot([makeSkill('refreshed-project-skill', 'project')]),
    };
    rerender(<SkillsPanel compact={false} />);
    expect(listScroll!.scrollTop).toBe(320);

    mocks.workspaceContextState.selectedContext = ubuntuGlobal;
    rerender(<SkillsPanel compact={false} />);

    expect(container.querySelector<HTMLDivElement>('.flex-1.overflow-auto')).toBe(listScroll);
    expect(listScroll!.scrollTop).toBe(0);
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

  it('filters Global and Project sections with their own associated Agent projections', async () => {
    mocks.workspaceContextState.selectedContext = ubuntuProject;
    const codex = makeResolvedAgent({ id: 'codex', displayName: 'Codex' });
    const cursor = makeResolvedAgent({ id: 'cursor', displayName: 'Cursor' });
    const globalSnapshot = snapshot([
      {
        ...makeSkill('global-skill'),
        agents: ['cursor'],
        associatedAgents: ['codex'],
      },
    ]);
    globalSnapshot.agents = [codex];
    const projectSnapshot = snapshot([
      {
        ...makeSkill('project-skill', 'project'),
        agents: ['codex'],
        associatedAgents: ['cursor'],
      },
    ]);
    projectSnapshot.agents = [cursor];
    mocks.skillsDataState.snapshots = {
      'wsl:ubuntu/global': globalSnapshot,
      'wsl:ubuntu/project:project-a': projectSnapshot,
    };

    render(<SkillsPanel compact={false} />);

    expect(screen.getByTestId('filterable-agents').textContent).toBe('codex,cursor');
    fireEvent.click(screen.getByTestId('filter-agent:cursor'));

    await waitFor(() => {
      expect(screen.queryByTestId('repair:global:global-skill')).toBeNull();
      expect(screen.getByTestId('repair:project:project-skill')).toBeDefined();
    });
  });

  it('supports an Agent whose literal ID is all', async () => {
    const allAgent = makeResolvedAgent({ id: 'all', displayName: 'All Tools' });
    const hostSnapshot = snapshot([
      { ...makeSkill('matched'), associatedAgents: ['all'] },
      { ...makeSkill('unrelated'), associatedAgents: ['codex'] },
    ]);
    hostSnapshot.agents = [allAgent];
    mocks.skillsDataState.snapshots = { 'host/global': hostSnapshot };

    render(<SkillsPanel compact={false} />);
    fireEvent.click(screen.getByTestId('filter-agent:all'));

    await waitFor(() => {
      expect(screen.getByTestId('selected-agent').textContent).toBe('all');
      expect(screen.getByTestId('repair:global:matched')).toBeDefined();
      expect(screen.queryByTestId('repair:global:unrelated')).toBeNull();
    });
  });

  it('preserves a still-valid Agent across Context changes and clears an invalid one', async () => {
    const codex = makeResolvedAgent({ id: 'codex', displayName: 'Codex' });
    const cursor = makeResolvedAgent({ id: 'cursor', displayName: 'Cursor' });
    const hostSnapshot = snapshot([{ ...makeSkill('host-skill'), associatedAgents: ['codex'] }]);
    hostSnapshot.agents = [codex];
    const ubuntuSnapshot = snapshot([{ ...makeSkill('ubuntu-skill'), associatedAgents: ['codex'] }]);
    ubuntuSnapshot.agents = [codex];
    const cursorSnapshot = snapshot([{ ...makeSkill('cursor-skill'), associatedAgents: ['cursor'] }]);
    cursorSnapshot.agents = [cursor];
    mocks.skillsDataState.snapshots = {
      'host/global': hostSnapshot,
      'wsl:ubuntu/global': ubuntuSnapshot,
    };

    const { rerender } = render(<SkillsPanel compact={false} />);
    fireEvent.click(screen.getByTestId('filter-agent:codex'));

    mocks.workspaceContextState.selectedContext = ubuntuGlobal;
    rerender(<SkillsPanel compact={false} />);
    expect(screen.getByTestId('selected-agent').textContent).toBe('codex');

    mocks.skillsDataState.snapshots['wsl:ubuntu/global'] = cursorSnapshot;
    rerender(<SkillsPanel compact={false} />);

    await waitFor(() => {
      expect(screen.getByTestId('selected-agent').textContent).toBe('no-agent');
    });
  });

  it('keeps the Agent filter while a previously unseen Context loads', () => {
    const codex = makeResolvedAgent({ id: 'codex', displayName: 'Codex' });
    const hostSnapshot = snapshot([{ ...makeSkill('host-skill'), associatedAgents: ['codex'] }]);
    hostSnapshot.agents = [codex];
    mocks.skillsDataState.snapshots = {
      'host/global': hostSnapshot,
    };

    const { rerender } = render(<SkillsPanel compact={false} />);
    fireEvent.click(screen.getByTestId('filter-agent:codex'));

    mocks.workspaceContextState.selectedContext = ubuntuGlobal;
    rerender(<SkillsPanel compact={false} />);

    expect(screen.getByTestId('selected-agent').textContent).toBe('codex');

    const ubuntuSnapshot = snapshot([{ ...makeSkill('ubuntu-skill'), associatedAgents: ['codex'] }]);
    ubuntuSnapshot.agents = [codex];
    mocks.skillsDataState.snapshots['wsl:ubuntu/global'] = ubuntuSnapshot;
    rerender(<SkillsPanel compact={false} />);

    expect(screen.getByTestId('selected-agent').textContent).toBe('codex');
  });

  it('keeps the selected Agent when the next Context has no Skills', () => {
    const codex = makeResolvedAgent({ id: 'codex', displayName: 'Codex' });
    const hostSnapshot = snapshot([{ ...makeSkill('host-skill'), associatedAgents: ['codex'] }]);
    hostSnapshot.agents = [codex];
    const ubuntuSnapshot = snapshot();
    ubuntuSnapshot.agents = [codex];
    mocks.skillsDataState.snapshots = {
      'host/global': hostSnapshot,
      'wsl:ubuntu/global': ubuntuSnapshot,
    };

    const { rerender } = render(<SkillsPanel compact={false} />);
    fireEvent.click(screen.getByTestId('filter-agent:codex'));

    mocks.workspaceContextState.selectedContext = ubuntuGlobal;
    rerender(<SkillsPanel compact={false} />);

    expect(screen.getByTestId('selected-agent').textContent).toBe('codex');
    expect(screen.getByText('global-empty-state')).toBeDefined();
  });

  it('uses the dedicated filtered-empty state in both full and compact layouts', async () => {
    const codex = makeResolvedAgent({ id: 'codex', displayName: 'Codex' });
    const cursor = makeResolvedAgent({ id: 'cursor', displayName: 'Cursor' });
    const hostSnapshot = snapshot([{ ...makeSkill('toolkit'), associatedAgents: ['codex'] }]);
    hostSnapshot.agents = [codex, cursor];
    mocks.skillsDataState.snapshots = { 'host/global': hostSnapshot };

    const { rerender } = render(<SkillsPanel compact={false} />);
    fireEvent.click(screen.getByTestId('filter-agent:codex'));
    expect(screen.queryByText('skill-filter-empty-state')).toBeNull();

    fireEvent.click(screen.getByTestId('set-missing-search'));
    await waitFor(() => {
      expect(screen.getByText('skill-filter-empty-state')).toBeDefined();
      expect(screen.queryByText('global-empty-state')).toBeNull();
    });

    rerender(<SkillsPanel compact />);
    expect(screen.getByTestId('toolbar-mode').textContent).toBe('compact');
    expect(screen.getByText('skill-filter-empty-state')).toBeDefined();
  });

  it('keeps separate Project and Global filtered-empty states in compact mode', async () => {
    mocks.workspaceContextState.selectedContext = ubuntuProject;
    const codex = makeResolvedAgent({ id: 'codex', displayName: 'Codex' });
    const globalSnapshot = snapshot([
      { ...makeSkill('global-skill'), associatedAgents: ['codex'] },
    ]);
    globalSnapshot.agents = [codex];
    const projectSnapshot = snapshot([
      { ...makeSkill('project-skill', 'project'), associatedAgents: ['codex'] },
    ]);
    projectSnapshot.agents = [codex];
    mocks.skillsDataState.snapshots = {
      'wsl:ubuntu/global': globalSnapshot,
      'wsl:ubuntu/project:project-a': projectSnapshot,
    };

    render(<SkillsPanel compact />);
    fireEvent.click(screen.getByTestId('set-missing-search'));

    await waitFor(() => {
      expect(screen.getAllByText('skill-filter-empty-state')).toHaveLength(2);
    });
  });

  it('checks updates only for the current filtered result', async () => {
    const codex = makeResolvedAgent({ id: 'codex', displayName: 'Codex' });
    const cursor = makeResolvedAgent({ id: 'cursor', displayName: 'Cursor' });
    const hostSnapshot = snapshot([
      { ...makeSkill('codex-skill'), associatedAgents: ['codex'] },
      { ...makeSkill('cursor-skill'), associatedAgents: ['cursor'] },
    ]);
    hostSnapshot.agents = [codex, cursor];
    mocks.skillsDataState.snapshots = { 'host/global': hostSnapshot };

    render(<SkillsPanel compact={false} />);
    fireEvent.click(screen.getByTestId('filter-agent:codex'));
    await waitFor(() => {
      expect(screen.queryByTestId('repair:global:cursor-skill')).toBeNull();
    });
    fireEvent.click(screen.getByTestId('check:global'));

    expect(mocks.skillsDataState.forceCheckUpdates).toHaveBeenCalledWith(hostGlobal, {
      kind: 'skills',
      skills: [{ context: hostGlobal, skillName: 'codex-skill' }],
    });
  });

  it('runs toolbar refresh as a passive sync so discovered source changes can be targeted', async () => {
    render(<SkillsPanel compact={false} />);

    fireEvent.click(screen.getByTestId('toolbar-sync'));

    expect(mocks.skillsDataState.syncSkills).toHaveBeenCalledWith(hostGlobal, { origin: 'passive' });
  });

  it('does not recheck on focus, remount, or unmount timer activity', async () => {
    const { unmount } = render(<SkillsPanel compact={false} />);
    await waitFor(() => {
      expect(mocks.skillsDataState.activateAutomaticChecks).toHaveBeenCalledWith(hostGlobal);
    });
    expect(mocks.skillsDataState.activateAutomaticChecks).toHaveBeenCalledTimes(1);

    window.dispatchEvent(new Event('focus'));
    window.dispatchEvent(new Event('focus'));
    expect(mocks.skillsDataState.activateAutomaticChecks).toHaveBeenCalledTimes(1);
    unmount();
  });

  it('waits for workspace refresh before activating the selected Context', async () => {
    let resolveRefresh!: () => void;
    mocks.skillsDataState.refreshWorkspace.mockImplementationOnce(() => new Promise<void>((resolve) => {
      resolveRefresh = resolve;
    }));
    render(<SkillsPanel compact={false} />);

    expect(mocks.skillsDataState.activateAutomaticChecks).not.toHaveBeenCalled();
    resolveRefresh();
    await waitFor(() => {
      expect(mocks.skillsDataState.activateAutomaticChecks).toHaveBeenCalledWith(hostGlobal);
    });
  });

  it('activates each newly selected Context once without a focus timer', async () => {
    const { rerender } = render(<SkillsPanel compact={false} />);
    await waitFor(() => expect(mocks.skillsDataState.activateAutomaticChecks).toHaveBeenCalledWith(hostGlobal));

    mocks.workspaceContextState.selectedContext = ubuntuGlobal;
    rerender(<SkillsPanel compact={false} />);
    await waitFor(() => expect(mocks.skillsDataState.activateAutomaticChecks).toHaveBeenCalledWith(ubuntuGlobal));
    expect(mocks.skillsDataState.activateAutomaticChecks).toHaveBeenCalledTimes(2);

    rerender(<SkillsPanel compact={false} />);
    expect(mocks.skillsDataState.activateAutomaticChecks).toHaveBeenCalledTimes(2);
  });

  it('does not start another check when the same Context is remounted', async () => {
    const first = render(<SkillsPanel compact={false} />);
    await waitFor(() => expect(mocks.skillsDataState.activateAutomaticChecks).toHaveBeenCalledWith(hostGlobal));
    first.unmount();
    render(<SkillsPanel compact={false} />);
    await act(async () => { await Promise.resolve(); });
    expect(mocks.skillsDataState.activateAutomaticChecks).toHaveBeenCalledTimes(2);
  });
});
