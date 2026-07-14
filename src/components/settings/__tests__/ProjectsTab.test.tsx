/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ContextRef, EnvironmentInfo, EnvironmentRef, ProjectInfo } from '@/bindings';
import { ProjectsTab } from '../ProjectsTab';
import { useMutationStore } from '@/stores/mutation';

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
  switchEnvironment: vi.fn(),
  refresh: vi.fn(),
  add: vi.fn(),
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
    pendingEnvironment: null as EnvironmentRef | null,
    contextRevision: 2,
  },
  projects: {
    projectsByEnvironment: { 'wsl:Ubuntu': [] } as Record<string, ProjectInfo[]>,
    loadStateByEnvironment: { 'wsl:Ubuntu': 'ready' as const },
    errorsByEnvironment: {},
  },
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open }));
vi.mock('@/stores/environment', () => ({
  environmentKey: (environment: { kind: string; distro_name?: string }) => (
    environment.kind === 'host' ? 'host' : `wsl:${environment.distro_name}`
  ),
  useEnvironmentStore: (selector: (state: unknown) => unknown) => selector({
    environments: mocks.environments,
  }),
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
    mocks.switchEnvironment.mockResolvedValue(undefined);
    mocks.workspace.pendingEnvironment = null;
    mocks.workspace.contextRevision = 2;
    mocks.projects.projectsByEnvironment = { 'wsl:Ubuntu': [project] };
    mocks.projects.loadStateByEnvironment = { 'wsl:Ubuntu': 'ready' };
    mocks.projects.errorsByEnvironment = {};
    useMutationStore.setState({
      revision: 0,
      activeMutation: null,
      cancelling: false,
      loading: false,
    });
  });

  it('adds the raw picker path to the committed environment', async () => {
    const rawPath = '\\\\wsl.localhost\\Ubuntu\\home\\me\\new-app';
    mocks.open.mockResolvedValue(rawPath);
    mocks.add.mockResolvedValue({ created: true });
    render(<ProjectsTab />);

    fireEvent.click(screen.getByRole('button', { name: 'settings.addProject' }));

    await waitFor(() => expect(mocks.add).toHaveBeenCalledWith(ubuntu, rawPath));
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

  it('disables every environment selector while a switch is pending', () => {
    mocks.workspace.pendingEnvironment = { kind: 'host' };
    render(<ProjectsTab />);

    expect((screen.getByRole('combobox', {
      name: 'context.environmentLabel',
    }) as HTMLSelectElement).disabled).toBe(true);
  });
});
