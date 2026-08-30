/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type {
  SkillLocationRef,
  EnvironmentInfo,
  EnvironmentRef,
  ProjectInfo,
} from '@/bindings';
import type { ProjectRemovalRequest } from '@/stores/project-removal';
import { useMutationStore } from '@/stores/mutation';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';
import { ContextSidebar } from '../ContextSidebar';

const mocks = vi.hoisted(() => ({
  open: vi.fn(),
  openConfigResource: vi.fn(),
  useProjectWorkspace: vi.fn(),
  selectGlobal: vi.fn(),
  selectProject: vi.fn(),
  refresh: vi.fn(),
  add: vi.fn(),
  captureProjectRemoval: vi.fn(),
  environments: [{
    environment: { kind: 'native' as const },
    displayName: 'Windows',
    status: 'available' as 'available' | 'connecting' | 'unavailable' | 'error' | undefined,
  }] as EnvironmentInfo[],
  workspace: {
    selectedContext: {
      environment: { kind: 'native' as const },
      scope: { scope: 'global' as const },
    } as SkillLocationRef,
    transition: { kind: 'idle' } as { kind: string; target?: EnvironmentRef },
    contextRevision: 0,
  },
  projectView: {
    projects: [] as ProjectInfo[],
    hasCompleteSnapshot: true,
    error: null as unknown,
    status: 'available' as 'available' | 'connecting' | 'unavailable' | 'error' | undefined,
  },
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open }));
vi.mock('@/hooks/useTauriApi', () => ({ openConfigResource: mocks.openConfigResource }));
vi.mock('@/stores/environment', () => ({
  environmentKey: (environment: EnvironmentRef) => (
    environment.kind === 'native' ? 'native' : `wsl:${environment.distro_name}`
  ),
  useEnvironmentStore: (selector: (state: { environments: EnvironmentInfo[] }) => unknown) => (
    selector({ environments: mocks.environments })
  ),
}));
vi.mock('@/hooks/useProjectWorkspace', () => ({
  useProjectWorkspace: (environment: EnvironmentRef) => {
    mocks.useProjectWorkspace(environment);
    return {
      ...mocks.projectView,
      refresh: mocks.refresh,
      add: mocks.add,
    };
  },
}));
vi.mock('@/stores/workspace-context', () => ({
  useWorkspaceContextStore: (selector: (state: unknown) => unknown) => selector({
    ...mocks.workspace,
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
  storage: { access: 'native', owner: { kind: 'native' } },
});

describe('ContextSidebar', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.refresh.mockResolvedValue({ status: 'succeeded' });
    Element.prototype.scrollIntoView = vi.fn();
    mocks.environments = [{
      environment: { kind: 'native' },
      displayName: 'Windows',
      status: 'available',
      revision: 1,
      error: null,
    }];
    mocks.workspace.selectedContext = {
      environment: { kind: 'native' },
      scope: { scope: 'global' },
    };
    mocks.workspace.transition = { kind: 'idle' };
    mocks.workspace.contextRevision = 0;
    mocks.projectView.projects = [];
    mocks.projectView.hasCompleteSnapshot = true;
    mocks.projectView.error = null;
    mocks.projectView.status = 'available';
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
    useInstallWizardSessionStore.setState({ revision: 0, active: false, loading: false });
  });

  it('leaves Environment switching to the main-window header', () => {
    mocks.environments = [mocks.environments[0], ubuntu];
    render(<ContextSidebar />);

    expect(screen.queryByRole('combobox', { name: 'context.environmentLabel' })).toBeNull();
  });

  it('leaves missing-project reconciliation to ProjectWorkspace', () => {
    mocks.workspace.selectedContext = {
      environment: { kind: 'native' },
      scope: { scope: 'project', project_id: 'missing-project' },
    };
    mocks.projectView.projects = [project('another-project')];

    render(<ContextSidebar />);

    expect(mocks.selectGlobal).not.toHaveBeenCalled();
  });

  it('renders every project and selects by stable ID', () => {
    mocks.projectView.projects = [project('a'), project('b'), project('c')];
    render(<ContextSidebar />);

    fireEvent.click(screen.getByText('C:\\Code\\b'));

    expect(mocks.selectProject).toHaveBeenCalledWith('b');
    expect(screen.getByText('C:\\Code\\a')).toBeDefined();
    expect(screen.getByText('C:\\Code\\c')).toBeDefined();
  });

  it('keeps Global fixed while only projects scroll', () => {
    mocks.environments = [mocks.environments[0], ubuntu];
    mocks.projectView.projects = [project('a')];
    const { container } = render(<ContextSidebar />);

    const projectScroll = screen.getByTestId('context-sidebar-scroll');
    const globalButton = screen.getByRole('button', { name: /context.global/ });

    expect(projectScroll.contains(globalButton)).toBe(false);
    expect(container.querySelectorAll('[data-testid="context-sidebar-scroll"]')).toHaveLength(1);
  });

  it('uses sibling buttons with full-text tooltips for project rows', () => {
    mocks.projectView.projects = [project('a')];
    const { container } = render(<ContextSidebar />);

    const row = container.querySelector('[data-project-id="a"]');
    expect(row).not.toBeNull();
    const buttons = within(row as HTMLElement).getAllByRole('button');
    expect(buttons).toHaveLength(3);
    expect(buttons.every((button) => button.querySelector('button') === null)).toBe(true);
    expect(buttons[0].tagName).toBe('BUTTON');
    expect(screen.getByText('a').getAttribute('title')).toBe('a');
    expect(screen.getByText('C:\\Code\\a').getAttribute('title')).toBe('C:\\Code\\a');
  });

  it('opens a project through its backend-owned context resource', () => {
    mocks.projectView.projects = [project('a')];
    const { container } = render(<ContextSidebar />);

    const row = container.querySelector('[data-project-id="a"]') as HTMLElement;
    fireEvent.click(within(row).getByRole('button', { name: 'context.openInExplorer' }));

    expect(mocks.openConfigResource).toHaveBeenCalledWith({
      environment: { kind: 'native' },
      scope: { scope: 'project', project_id: 'a' },
    }, 'contextRoot');
  });

  it('restores focus to the next project, then Global when no project remains', async () => {
    mocks.projectView.projects = [project('a'), project('b'), project('c')];
    const firstView = render(<ContextSidebar />);
    const rowB = firstView.container.querySelector('[data-project-id="b"]') as HTMLElement;

    fireEvent.click(within(rowB).getByRole('button', { name: 'context.remove' }));
    fireEvent.click(screen.getByRole('button', { name: 'complete-removal' }));

    const rowC = firstView.container.querySelector('[data-project-id="c"]') as HTMLElement;
    await waitFor(() => {
      expect(document.activeElement).toBe(within(rowC).getAllByRole('button')[0]);
    });
    firstView.unmount();

    mocks.projectView.projects = [project('only')];
    const secondView = render(<ContextSidebar />);
    const onlyRow = secondView.container.querySelector('[data-project-id="only"]') as HTMLElement;
    fireEvent.click(within(onlyRow).getByRole('button', { name: 'context.remove' }));
    fireEvent.click(screen.getByRole('button', { name: 'complete-removal' }));

    await waitFor(() => {
      expect(document.activeElement).toBe(screen.getByRole('button', { name: /context.global/ }));
    });
  });

  it('passes the raw picker path and captured environment to ProjectWorkspace', async () => {
    const rawPath = '\\\\wsl.localhost\\Ubuntu\\home\\me\\app';
    mocks.environments = [mocks.environments[0], ubuntu];
    mocks.workspace.selectedContext = {
      environment: ubuntu.environment,
      scope: { scope: 'global' },
    };
    mocks.projectView.projects = [];
    mocks.open.mockResolvedValue(rawPath);
    mocks.add.mockResolvedValue({ created: true });
    render(<ContextSidebar />);

    fireEvent.click(screen.getByRole('button', { name: 'context.addProject' }));

    await waitFor(() => expect(mocks.add).toHaveBeenCalledWith(rawPath));
    expect(mocks.useProjectWorkspace).toHaveBeenCalledWith(ubuntu.environment);
  });

  it('disables project registration until the first complete snapshot is available', () => {
    mocks.projectView.hasCompleteSnapshot = false;

    render(<ContextSidebar />);

    expect((screen.getByRole('button', { name: 'context.addProject' }) as HTMLButtonElement).disabled)
      .toBe(true);
  });

  it('keeps a complete project list visible when background refresh fails', () => {
    mocks.projectView.projects = [project('a')];
    mocks.projectView.hasCompleteSnapshot = true;
    mocks.projectView.error = { kind: 'custom', data: { message: 'refresh failed' } };

    render(<ContextSidebar />);

    expect(screen.getByText('C:\\Code\\a')).toBeDefined();
    fireEvent.click(screen.getByRole('button', { name: 'context.environmentRetry' }));
    expect(mocks.refresh).toHaveBeenCalledTimes(1);
  });

  it('keeps a complete project list visible while its Environment is unavailable', () => {
    mocks.projectView.projects = [project('a')];
    mocks.projectView.status = 'unavailable';

    render(<ContextSidebar />);

    expect(screen.getByText('context.environmentUnavailable')).toBeDefined();
    expect(screen.getByText('C:\\Code\\a')).toBeDefined();
  });

  it('disables project writes while the global Environment control owns switching', () => {
    mocks.environments = [mocks.environments[0], ubuntu];
    useMutationStore.setState({
      activeMutation: {
        kind: 'install',
        target: { kind: 'skillLocation', environment: { kind: 'native' }, scope: { scope: 'global' } },
        id: 'mutation-1',
        phase: 'preparing',
        progress: null,
        cancelable: true,
      },
    });
    render(<ContextSidebar />);

    expect((screen.getByRole('button', { name: 'context.addProject' }) as HTMLButtonElement).disabled)
      .toBe(true);
    expect(screen.queryByRole('combobox', { name: 'context.environmentLabel' })).toBeNull();
  });

  it('keeps Context selection available while the wizard blocks project writes', () => {
    mocks.projectView.projects = [project('a')];
    useInstallWizardSessionStore.setState({ revision: 1, active: true });
    render(<ContextSidebar />);

    fireEvent.click(screen.getByText('C:\\Code\\a'));

    expect(mocks.selectProject).toHaveBeenCalledWith('a');
    expect((screen.getByRole('button', { name: 'context.addProject' }) as HTMLButtonElement).disabled)
      .toBe(true);
  });
});
