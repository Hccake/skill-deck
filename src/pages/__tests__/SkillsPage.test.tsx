/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, waitFor } from '@testing-library/react';
import { useEffect } from 'react';
import { SkillsPage } from '../SkillsPage';
import type { ContextRef, EnvironmentRef, InstalledSkill } from '@/bindings';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

const mocks = vi.hoisted(() => ({
  contextState: {
    selectedContext: 'global',
    selectedContextRef: {
      environment: { kind: 'host' },
      scope: { scope: 'global' },
    } as ContextRef,
    hasExplicitContext: false,
    projects: [] as string[],
  },
  environmentState: {
    selectedEnvironment: { kind: 'host' as const } as EnvironmentRef,
    projectsByEnvironment: {} as Record<string, Array<{
      id: string;
      nativePath: string;
      displayName: string | null;
      order: number | null;
      suppressCrossStorageWarning: boolean;
    }>>,
  },
  skillsDataState: {
    globalSkills: [] as InstalledSkill[],
    projectSkills: [] as InstalledSkill[],
    allAgents: [] as Array<{ id: string; name: string }>,
    checkingUpdateScopes: new Set<string>(),
    updatingSkills: new Map<string, 'queued' | 'updating' | 'done' | 'failed'>(),
    forceCheckUpdates: vi.fn(),
    updateSkill: vi.fn(),
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
    manageAgentsScope: 'global',
    copySkill: null as InstalledSkill | null,
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
}));

function makeSkill(name: string, overrides: Partial<InstalledSkill> = {}): InstalledSkill {
  return {
    name,
    description: `${name} description`,
    path: `/skills/${name}`,
    canonicalPath: `/canonical/${name}`,
    scope: 'global',
    agents: [],
    hasUpdate: false,
    ...overrides,
  };
}

vi.mock('@/stores/context', () => ({
  useContextStore: (selector: (state: typeof mocks.contextState) => unknown) => selector(mocks.contextState),
}));

vi.mock('@/stores/environment', () => ({
  environmentKey: (environment: { kind: string; distro_name?: string }) => (
    environment.kind === 'host' ? 'host' : `wsl:${environment.distro_name}`
  ),
  useEnvironmentStore: (selector: (state: typeof mocks.environmentState) => unknown) => selector(mocks.environmentState),
}));

vi.mock('@/stores/skills-data', () => ({
  useSkillsDataStore: (selector: (state: typeof mocks.skillsDataState) => unknown) => selector(mocks.skillsDataState),
}));

vi.mock('@/stores/skill-detail', () => ({
  useSkillDetailStore: (selector: (state: typeof mocks.skillDetailState) => unknown) => selector(mocks.skillDetailState),
}));

vi.mock('@/stores/skill-dialog', () => ({
  useSkillDialogStore: (selector: (state: typeof mocks.skillDialogState) => unknown) => selector(mocks.skillDialogState),
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
  }: {
    skill: InstalledSkill;
    updateStatus?: string;
    isCheckingUpdates?: boolean;
    onCheckUpdates?: () => void;
  }) => (
    <button
      type="button"
      data-skill-name={skill.name}
      data-update-status={updateStatus ?? 'idle'}
      data-checking-updates={isCheckingUpdates ? 'true' : 'false'}
      onClick={onCheckUpdates}
    >
      skill-detail-panel
    </button>
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
    mocks.contextState.selectedContext = 'global';
    mocks.contextState.selectedContextRef = {
      environment: { kind: 'host' },
      scope: { scope: 'global' },
    };
    mocks.contextState.hasExplicitContext = false;
    mocks.environmentState.selectedEnvironment = { kind: 'host' };
    mocks.environmentState.projectsByEnvironment = {};
    mocks.skillsDataState.globalSkills = [];
    mocks.skillsDataState.projectSkills = [];
    mocks.skillsDataState.allAgents = [];
    mocks.skillsDataState.checkingUpdateScopes = new Set();
    mocks.skillsDataState.updatingSkills = new Map();
    mocks.skillsDataState.forceCheckUpdates.mockReset();
    mocks.skillsDataState.updateSkill.mockReset();
    mocks.skillDetailState.selectedSkillRef = null;
    mocks.skillDetailState.skillContent = null;
    mocks.skillDetailState.loadingContent = false;
    mocks.skillDetailState.deselectSkill.mockReset();
    mocks.skillDetailState.reloadContent.mockReset();
    mocks.skillDialogState.openDelete.mockReset();
    mocks.skillDialogState.copySkill = null;
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
    mocks.skillsDataState.globalSkills = [makeSkill('test-skill')];

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
    mocks.skillsDataState.globalSkills = [makeSkill('test-skill')];

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
    mocks.skillsDataState.globalSkills = [makeSkill('test-skill')];

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
    mocks.skillsDataState.globalSkills = [makeSkill('toolkit')];
    mocks.skillsDataState.updatingSkills = new Map([['global:toolkit', 'updating']]);

    const { getByText } = render(<SkillsPage />);

    expect(getByText('skill-detail-panel').getAttribute('data-update-status')).toBe('updating');
  });

  it('wires detail check-updates to the selected skill scope', () => {
    mocks.skillDetailState.selectedSkillRef = {
      name: 'toolkit',
      scope: 'global',
    };
    mocks.skillsDataState.globalSkills = [makeSkill('toolkit')];
    mocks.skillsDataState.checkingUpdateScopes = new Set(['global']);

    const { getByText } = render(<SkillsPage />);
    const detailButton = getByText('skill-detail-panel');

    expect(detailButton.getAttribute('data-checking-updates')).toBe('true');

    fireEvent.click(detailButton);

    expect(mocks.skillsDataState.forceCheckUpdates).toHaveBeenCalledWith('global');
  });

  it('uses the explicit environment key for detail update progress', () => {
    const context: ContextRef = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    };
    mocks.contextState.hasExplicitContext = true;
    mocks.contextState.selectedContextRef = context;
    mocks.skillDetailState.selectedSkillRef = {
      name: 'toolkit',
      scope: 'global',
    };
    mocks.skillsDataState.globalSkills = [makeSkill('toolkit')];
    mocks.skillsDataState.checkingUpdateScopes = new Set([JSON.stringify(context)]);

    const { getByText } = render(<SkillsPage />);

    expect(getByText('skill-detail-panel').getAttribute('data-checking-updates')).toBe('true');
  });

  it('shows copy targets from the selected WSL environment', () => {
    mocks.contextState.hasExplicitContext = true;
    mocks.contextState.selectedContext = '/home/me/current';
    mocks.contextState.selectedContextRef = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'current' },
    };
    mocks.environmentState.selectedEnvironment = {
      kind: 'wsl',
      distro_name: 'Ubuntu',
    };
    mocks.environmentState.projectsByEnvironment = {
      'wsl:Ubuntu': [
        {
          id: 'current',
          nativePath: '/home/me/current',
          displayName: null,
          order: null,
          suppressCrossStorageWarning: false,
        },
        {
          id: 'target',
          nativePath: '/home/me/target',
          displayName: null,
          order: null,
          suppressCrossStorageWarning: false,
        },
      ],
    };
    mocks.skillDialogState.copySkill = makeSkill('toolkit', { scope: 'project' });

    const { getByText, queryByText } = render(<SkillsPage />);

    expect(getByText('/home/me/target')).toBeDefined();
    expect(queryByText('/home/me/current')).toBeNull();
  });

  it('derives the selected skill from the shared skills store', () => {
    mocks.skillDetailState.selectedSkillRef = {
      name: 'toolkit',
      scope: 'global',
    };
    mocks.skillsDataState.globalSkills = [
      makeSkill('toolkit', { updatedAt: '2026-04-07T12:00:00.000Z' }),
    ];

    const { getByText } = render(<SkillsPage />);

    expect(getByText('skill-detail-panel').getAttribute('data-skill-name')).toBe('toolkit');
  });

  it('deselects a stale skill identity when it no longer resolves in the current context', async () => {
    mocks.skillDetailState.selectedSkillRef = {
      name: 'missing-skill',
      scope: 'global',
    };
    mocks.skillsDataState.globalSkills = [];

    render(<SkillsPage />);

    await waitFor(() => {
      expect(mocks.skillDetailState.deselectSkill).toHaveBeenCalledTimes(1);
    });
  });
});
