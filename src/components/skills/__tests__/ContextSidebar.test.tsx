/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { ContextRef, EnvironmentInfo, EnvironmentRef, ProjectInfo } from '@/bindings';
import type { ProjectRemovalRequest } from '@/stores/project-removal';
import { useMutationStore } from '@/stores/mutation';
import { ContextSidebar } from '../ContextSidebar';

const mocks = vi.hoisted(() => ({
  open: vi.fn(),
  openConfigResource: vi.fn(),
  switchEnvironment: vi.fn(),
  selectGlobal: vi.fn(),
  selectProject: vi.fn(),
  refresh: vi.fn(),
  add: vi.fn(),
  captureProjectRemoval: vi.fn(),
  environments: [{
    environment: { kind: 'host' as const },
    displayName: 'Windows',
    status: 'available' as const,
  }] as EnvironmentInfo[],
  workspace: {
    selectedContext: {
      environment: { kind: 'host' as const },
      scope: { scope: 'global' as const },
    } as ContextRef,
    pendingEnvironment: null as EnvironmentRef | null,
    contextRevision: 0,
  },
  projects: {
    projectsByEnvironment: { host: [] } as Record<string, ProjectInfo[]>,
    loadStateByEnvironment: { host: 'ready' as const } as Record<string, 'idle' | 'loading' | 'ready' | 'error'>,
    errorsByEnvironment: {} as Record<string, unknown>,
  },
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open }));
vi.mock('@/hooks/useTauriApi', () => ({ openConfigResource: mocks.openConfigResource }));
vi.mock('@/stores/environment', () => ({
  environmentKey: (environment: EnvironmentRef) => (
    environment.kind === 'host' ? 'host' : `wsl:${environment.distro_name}`
  ),
  useEnvironmentStore: (selector: (state: { environments: EnvironmentInfo[] }) => unknown) => (
    selector({ environments: mocks.environments })
  ),
}));
vi.mock('@/stores/projects', () => ({
  useProjectStore: (selector: (state: unknown) => unknown) => selector({
    ...mocks.projects,
    refresh: mocks.refresh,
    add: mocks.add,
  }),
}));
vi.mock('@/stores/workspace-context', () => ({
  useWorkspaceContextStore: (selector: (state: unknown) => unknown) => selector({
    ...mocks.workspace,
    switchEnvironment: mocks.switchEnvironment,
    selectGlobal: mocks.selectGlobal,
    selectProject: mocks.selectProject,
  }),
}));
vi.mock('@/stores/project-removal', () => ({
  captureProjectRemoval: (...args: unknown[]) => mocks.captureProjectRemoval(...args),
}));
vi.mock('@/components/projects/RemoveProjectDialog', () => ({
  RemoveProjectDialog: ({
    request,
    onRemoved,
  }: {
    request: ProjectRemovalRequest | null;
    onRemoved?: (request: ProjectRemovalRequest) => void;
  }) => request ? (
    <button type="button" onClick={() => onRemoved?.(request)}>
      complete-removal
    </button>
  ) : null,
}));

const ubuntu: EnvironmentInfo = {
  environment: { kind: 'wsl', distro_name: 'Ubuntu' },
  displayName: 'Ubuntu',
  status: 'available',
  revision: 1,
  error: null,
};
const project = (id: string): ProjectInfo => ({
  binding: {
    id,
    nativePath: `C:\\Code\\${id}`,
    displayName: null,
    order: null,
    suppressCrossStorageWarning: false,
  },
  storage: { access: 'native', owner: { kind: 'host' } },
});

describe('ContextSidebar', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    Element.prototype.scrollIntoView = vi.fn();
    mocks.switchEnvironment.mockResolvedValue(undefined);
    mocks.environments = [{
      environment: { kind: 'host' },
      displayName: 'Windows',
      status: 'available',
      revision: 1,
      error: null,
    }];
    mocks.workspace.selectedContext = {
      environment: { kind: 'host' },
      scope: { scope: 'global' },
    };
    mocks.workspace.pendingEnvironment = null;
    mocks.workspace.contextRevision = 0;
    mocks.projects.projectsByEnvironment = { host: [] };
    mocks.projects.loadStateByEnvironment = { host: 'ready' };
    mocks.projects.errorsByEnvironment = {};
    mocks.captureProjectRemoval.mockImplementation((
      environment: EnvironmentRef,
      target: ProjectInfo,
      contextRevision: number,
    ) => ({
      environment,
      projectId: target.binding.id,
      projectName: target.binding.id,
      contextRevision,
    }));
    useMutationStore.setState({
      revision: 0,
      activeMutation: null,
      cancelling: false,
      loading: false,
    });
  });

  it('hides environment switching when only Host exists', () => {
    render(<ContextSidebar />);
    expect(screen.queryByRole('combobox', { name: 'context.environmentLabel' })).toBeNull();
  });

  it('switches environments through the workspace transaction', async () => {
    mocks.environments = [mocks.environments[0], ubuntu];
    render(<ContextSidebar />);

    fireEvent.click(screen.getByRole('combobox', { name: 'context.environmentLabel' }));
    fireEvent.click(await screen.findByRole('option', { name: 'Ubuntu' }));

    expect(mocks.switchEnvironment).toHaveBeenCalledWith(ubuntu.environment);
  });

  it('disables environment selection during a pending switch', () => {
    mocks.environments = [mocks.environments[0], ubuntu];
    mocks.workspace.pendingEnvironment = ubuntu.environment;
    render(<ContextSidebar />);

    expect((screen.getByRole('combobox', {
      name: 'context.environmentLabel',
    }) as HTMLSelectElement).disabled).toBe(true);
  });

  it('renders every project in one scrollable list and selects by stable ID', () => {
    mocks.projects.projectsByEnvironment = { host: [project('a'), project('b'), project('c')] };
    const { container } = render(<ContextSidebar />);

    fireEvent.click(screen.getByText('C:\\Code\\b'));

    expect(mocks.selectProject).toHaveBeenCalledWith('b');
    expect(screen.getByText('C:\\Code\\a')).toBeDefined();
    expect(screen.getByText('C:\\Code\\c')).toBeDefined();
    expect(container.querySelector('[data-testid="context-sidebar-scroll"]')?.classList)
      .toContain('overflow-y-auto');
  });

  it('keeps environment and Global fixed while only projects scroll', () => {
    mocks.environments = [mocks.environments[0], ubuntu];
    mocks.projects.projectsByEnvironment = { host: [project('a')] };
    const { container } = render(<ContextSidebar />);

    const projectScroll = screen.getByTestId('context-sidebar-scroll');
    const environmentSelect = screen.getByRole('combobox', { name: 'context.environmentLabel' });
    const globalButton = screen.getByRole('button', { name: /context.global/ });

    expect(projectScroll.contains(environmentSelect)).toBe(false);
    expect(projectScroll.contains(globalButton)).toBe(false);
    expect(container.querySelectorAll('[data-testid="context-sidebar-scroll"]')).toHaveLength(1);
  });

  it('uses sibling buttons, full-text tooltips, and large-list containment for project rows', () => {
    mocks.projects.projectsByEnvironment = { host: [project('a')] };
    const { container } = render(<ContextSidebar />);

    const row = container.querySelector('[data-project-id="a"]');
    expect(row).not.toBeNull();
    const buttons = within(row as HTMLElement).getAllByRole('button');
    expect(buttons).toHaveLength(3);
    expect(buttons.every((button) => button.querySelector('button') === null)).toBe(true);
    expect(buttons[0].tagName).toBe('BUTTON');
    expect(row?.classList.contains('project-context-item')).toBe(true);
    expect(screen.getByText('a').getAttribute('title')).toBe('a');
    expect(screen.getByText('C:\\Code\\a').getAttribute('title')).toBe('C:\\Code\\a');
  });

  it('opens a project through its backend-owned context resource', () => {
    mocks.projects.projectsByEnvironment = { host: [project('a')] };
    const { container } = render(<ContextSidebar />);

    const row = container.querySelector('[data-project-id="a"]') as HTMLElement;
    fireEvent.click(within(row).getByRole('button', { name: 'context.openInExplorer' }));

    expect(mocks.openConfigResource).toHaveBeenCalledWith({
      environment: { kind: 'host' },
      scope: { scope: 'project', project_id: 'a' },
    }, 'contextRoot');
  });

  it('restores focus to the next project, then Global when no project remains', async () => {
    mocks.projects.projectsByEnvironment = { host: [project('a'), project('b'), project('c')] };
    const firstView = render(<ContextSidebar />);
    const rowB = firstView.container.querySelector('[data-project-id="b"]') as HTMLElement;

    fireEvent.click(within(rowB).getByRole('button', { name: 'context.remove' }));
    fireEvent.click(screen.getByRole('button', { name: 'complete-removal' }));

    const rowC = firstView.container.querySelector('[data-project-id="c"]') as HTMLElement;
    await waitFor(() => {
      expect(document.activeElement).toBe(within(rowC).getAllByRole('button')[0]);
    });
    firstView.unmount();

    mocks.projects.projectsByEnvironment = { host: [project('only')] };
    const secondView = render(<ContextSidebar />);
    const onlyRow = secondView.container.querySelector('[data-project-id="only"]') as HTMLElement;
    fireEvent.click(within(onlyRow).getByRole('button', { name: 'context.remove' }));
    fireEvent.click(screen.getByRole('button', { name: 'complete-removal' }));

    await waitFor(() => {
      expect(document.activeElement).toBe(screen.getByRole('button', { name: /context.global/ }));
    });
  });

  it('passes the raw picker path and captured environment to ProjectStore', async () => {
    const rawPath = '\\\\wsl.localhost\\Ubuntu\\home\\me\\app';
    mocks.environments = [mocks.environments[0], ubuntu];
    mocks.workspace.selectedContext = {
      environment: ubuntu.environment,
      scope: { scope: 'global' },
    };
    mocks.projects.projectsByEnvironment = { 'wsl:ubuntu': [] };
    mocks.projects.loadStateByEnvironment = { 'wsl:ubuntu': 'ready' };
    mocks.open.mockResolvedValue(rawPath);
    mocks.add.mockResolvedValue({ created: true });
    render(<ContextSidebar />);

    fireEvent.click(screen.getByRole('button', { name: 'context.addProject' }));

    await waitFor(() => expect(mocks.add).toHaveBeenCalledWith(ubuntu.environment, rawPath));
  });

  it('disables project writes without blocking environment browsing', () => {
    mocks.environments = [mocks.environments[0], ubuntu];
    useMutationStore.setState({
      activeMutation: {
        kind: 'install',
        context: { environment: { kind: 'host' }, scope: { scope: 'global' } },
        id: 'mutation-1',
        phase: 'preparing',
        progress: null,
        cancelable: true,
      },
    });
    render(<ContextSidebar />);

    expect((screen.getByRole('button', { name: 'context.addProject' }) as HTMLButtonElement).disabled)
      .toBe(true);
    expect((screen.getByRole('combobox', { name: 'context.environmentLabel' }) as HTMLSelectElement).disabled)
      .toBe(false);
  });
});
