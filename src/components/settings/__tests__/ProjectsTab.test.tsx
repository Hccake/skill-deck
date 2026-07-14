/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { ProjectsTab } from '../ProjectsTab';
import { useMutationStore } from '@/stores/mutation';

const mocks = vi.hoisted(() => ({
  open: vi.fn(),
  mapEnvironmentPath: vi.fn(),
  selectEnvironment: vi.fn(),
  refreshProjects: vi.fn(),
  addProject: vi.fn(),
  removeProject: vi.fn(),
  state: {
    environments: [
      { environment: { kind: 'host' as const }, displayName: 'Windows', status: 'available' as const },
      { environment: { kind: 'wsl' as const, distro_name: 'Ubuntu' }, displayName: 'Ubuntu', status: 'available' as const },
    ],
    selectedEnvironment: { kind: 'wsl' as const, distro_name: 'Ubuntu' },
    projectsByEnvironment: {
      'wsl:Ubuntu': [{
        id: 'project-1',
        nativePath: '/home/me/app',
        displayName: null,
        order: null,
        suppressCrossStorageWarning: false,
      }],
    },
    projectsLoaded: { 'wsl:Ubuntu': true },
    errors: {} as Record<string, string | null>,
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
  useEnvironmentStore: () => ({
    ...mocks.state,
    selectEnvironment: mocks.selectEnvironment,
    refreshProjects: mocks.refreshProjects,
    addProject: mocks.addProject,
    removeProject: mocks.removeProject,
  }),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  mapEnvironmentPath: (...args: unknown[]) => mocks.mapEnvironmentPath(...args),
}));

describe('ProjectsTab', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.open.mockResolvedValue('\\\\wsl.localhost\\Ubuntu\\home\\me\\new-app');
    mocks.mapEnvironmentPath.mockResolvedValue('/home/me/new-app');
    mocks.addProject.mockResolvedValue([]);
    mocks.removeProject.mockResolvedValue([]);
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
  });

  it('shows and manages projects in the selected environment', async () => {
    render(<ProjectsTab />);

    expect(screen.getByRole('combobox', { name: 'context.environmentLabel' })).toBeDefined();
    expect(screen.getByText('/home/me/app')).toBeDefined();

    fireEvent.click(screen.getByRole('button', { name: 'settings.addProject' }));
    await waitFor(() => expect(mocks.mapEnvironmentPath).toHaveBeenCalledWith(
      { kind: 'wsl', distro_name: 'Ubuntu' },
      '\\\\wsl.localhost\\Ubuntu\\home\\me\\new-app',
    ));
    expect(mocks.addProject).toHaveBeenCalledWith(
      '/home/me/new-app',
      { kind: 'wsl', distro_name: 'Ubuntu' },
    );

    fireEvent.click(screen.getByRole('button', { name: 'settings.removeProject' }));
    expect(mocks.removeProject).toHaveBeenCalledWith(
      'project-1',
      { kind: 'wsl', distro_name: 'Ubuntu' },
    );
  });

  it('disables project writes while keeping environment selection available', () => {
    useMutationStore.setState({
      activeMutation: {
        kind: 'update',
        context: { environment: { kind: 'host' }, scope: { scope: 'global' } },
        statusText: 'Updating',
        cancelable: true,
      },
    });

    render(<ProjectsTab />);

    expect((screen.getByRole('button', { name: 'settings.addProject' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'settings.removeProject' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('combobox', { name: 'context.environmentLabel' }) as HTMLSelectElement).disabled).toBe(false);
  });
});
