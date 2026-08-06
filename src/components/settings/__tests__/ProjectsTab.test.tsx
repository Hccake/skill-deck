/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ContextRef, EnvironmentInfo, EnvironmentRef, ProjectInfo } from '@/bindings';
import { ProjectsTab } from '../ProjectsTab';
import { useMutationStore } from '@/stores/mutation';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';

const ubuntu = { kind: 'wsl' as const, distro_name: 'Ubuntu' };
const project: ProjectInfo = {
  binding: {
    id: 'project-1',
    nativePath: '/home/me/app',
    displayName: null,
    order: null,
    suppressCrossStorageWarning: false,
  },
  storage: { access: 'native', owner: ubuntu },
};
const mocks = vi.hoisted(() => ({
  open: vi.fn(),
  useProjectWorkspace: vi.fn(),
  refresh: vi.fn(),
  add: vi.fn(),
  toastError: vi.fn(),
  captureProjectRemoval: vi.fn(),
  environments: [
    { environment: { kind: 'host' as const }, displayName: 'Windows', status: 'available' as const },
    { environment: { kind: 'wsl' as const, distro_name: 'Ubuntu' }, displayName: 'Ubuntu', status: 'available' as const },
  ] as EnvironmentInfo[],
  workspace: {
    selectedContext: {
      environment: { kind: 'wsl' as const, distro_name: 'Ubuntu' },
      scope: { scope: 'global' as const },
    } as ContextRef,
    transition: { kind: 'idle' } as { kind: string; target?: EnvironmentRef },
    contextRevision: 2,
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
vi.mock('sonner', () => ({ toast: { error: mocks.toastError } }));
vi.mock('@/stores/environment', () => ({
  useEnvironmentStore: (selector: (state: unknown) => unknown) => selector({
    environments: mocks.environments,
  }),
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
  }),
}));
vi.mock('@/stores/project-removal', () => ({
  captureProjectRemoval: (...args: unknown[]) => mocks.captureProjectRemoval(...args),
}));
vi.mock('@/components/projects/RemoveProjectDialog', () => ({
  RemoveProjectDialog: ({ request }: { request: { projectId: string } | null }) => (
    request ? <div data-testid="remove-project-dialog">{request.projectId}</div> : null
  ),
}));

describe('ProjectsTab', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.refresh.mockResolvedValue({ status: 'succeeded' });
    mocks.workspace.transition = { kind: 'idle' };
    mocks.workspace.contextRevision = 2;
    mocks.projectView.projects = [project];
    mocks.projectView.hasCompleteSnapshot = true;
    mocks.projectView.error = null;
    mocks.projectView.status = 'available';
    useMutationStore.setState({
      revision: 0,
      activeMutation: null,
      cancelling: false,
      loading: false,
    });
    useInstallWizardSessionStore.setState({ revision: 0, active: false, loading: false });
  });

  it('adds the raw picker path to the committed environment', async () => {
    const rawPath = '\\\\wsl.localhost\\Ubuntu\\home\\me\\new-app';
    mocks.open.mockResolvedValue(rawPath);
    mocks.add.mockResolvedValue({ created: true });
    render(<ProjectsTab />);

    fireEvent.click(screen.getByRole('button', { name: 'settings.addProject' }));

    await waitFor(() => expect(mocks.add).toHaveBeenCalledWith(rawPath));
    expect(mocks.useProjectWorkspace).toHaveBeenCalledWith(ubuntu);
  });

  it('disables project registration until the first complete snapshot is available', () => {
    mocks.projectView.hasCompleteSnapshot = false;

    render(<ProjectsTab />);

    expect((screen.getByRole('button', { name: 'settings.addProject' }) as HTMLButtonElement).disabled)
      .toBe(true);
  });

  it('shows Environment unavailability instead of project loading', () => {
    mocks.projectView.hasCompleteSnapshot = false;
    mocks.projectView.status = 'unavailable';

    render(<ProjectsTab />);

    expect(screen.getByText('context.environmentUnavailable')).toBeDefined();
    expect(screen.queryByText('common.loading')).toBeNull();
  });

  it('keeps the complete project list visible when a background refresh fails', () => {
    mocks.projectView.hasCompleteSnapshot = true;
    mocks.projectView.error = { kind: 'custom', data: { message: 'refresh failed' } };

    render(<ProjectsTab />);

    expect(screen.getByText('/home/me/app')).toBeDefined();
    fireEvent.click(screen.getByRole('button', { name: 'context.environmentRetry' }));
    expect(mocks.refresh).toHaveBeenCalledTimes(1);
  });

  it('reports an add failure without replacing the project catalog', async () => {
    mocks.open.mockResolvedValue('/home/me/new-app');
    mocks.add.mockResolvedValue({
      status: 'failed',
      error: { kind: 'custom', data: { message: 'add failed' } },
    });

    render(<ProjectsTab />);
    fireEvent.click(screen.getByRole('button', { name: 'settings.addProject' }));

    await waitFor(() => expect(mocks.toastError).toHaveBeenCalledWith('settings.addProjectError'));
    expect(screen.getByText('/home/me/app')).toBeDefined();
  });

  it('captures removal identity and revision before opening the shared dialog', () => {
    const request = {
      environment: ubuntu,
      projectId: 'project-1',
      projectName: 'app',
      contextRevision: 2,
    };
    mocks.captureProjectRemoval.mockReturnValue(request);
    render(<ProjectsTab />);

    fireEvent.click(screen.getByRole('button', { name: 'settings.removeProject' }));

    expect(mocks.captureProjectRemoval).toHaveBeenCalledWith(ubuntu, project, 2);
    expect(screen.getByTestId('remove-project-dialog').textContent).toBe('project-1');
  });

  it('leaves Environment switching to the main-window header', () => {
    mocks.workspace.transition = { kind: 'switchEnvironment', target: { kind: 'host' } };
    render(<ProjectsTab />);

    expect(screen.queryByRole('combobox', { name: 'context.environmentLabel' })).toBeNull();
  });

  it('keeps project registration read-only while the install wizard is open', () => {
    useInstallWizardSessionStore.setState({ revision: 1, active: true });
    render(<ProjectsTab />);

    expect((screen.getByRole('button', { name: 'settings.addProject' }) as HTMLButtonElement).disabled)
      .toBe(true);
    expect((screen.getByRole('button', { name: 'settings.removeProject' }) as HTMLButtonElement).disabled)
      .toBe(true);
  });
});
