/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { ContextSidebar } from '../ContextSidebar';
import zhCN from '@/i18n/locales/zh-CN.json';
import type { EnvironmentInfo, EnvironmentRef } from '@/bindings';

const mocks = vi.hoisted(() => ({
  open: vi.fn(),
  mapEnvironmentPath: vi.fn(),
  openInExplorer: vi.fn(),
  selectEnvironment: vi.fn(),
  refreshProjects: vi.fn(),
  addProject: vi.fn(),
  removeProject: vi.fn(),
  selectContextRef: vi.fn(),
  environmentState: {
    environments: [{
      environment: { kind: 'host' as const },
      displayName: 'Windows',
      status: 'available' as const,
    }] as EnvironmentInfo[],
    selectedEnvironment: { kind: 'host' as const } as EnvironmentRef,
    projectsByEnvironment: {
      host: [],
    } as Record<string, Array<{
      id: string;
      nativePath: string;
      displayName: string | null;
      order: number | null;
      suppressCrossStorageWarning: boolean;
    }>>,
    projectsLoaded: { host: true } as Record<string, boolean>,
    discoveryState: 'ready' as const,
    errors: {} as Record<string, string | null>,
  },
  contextState: {
    selectedContext: 'global',
    projects: [] as string[],
    projectsLoaded: true,
    selectedContextRef: {
      environment: { kind: 'host' as const },
      scope: { scope: 'global' as const },
    },
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
  useEnvironmentStore: (selector?: (state: unknown) => unknown) => {
    const state = {
    ...mocks.environmentState,
    selectEnvironment: mocks.selectEnvironment,
    refreshProjects: mocks.refreshProjects,
    addProject: mocks.addProject,
    removeProject: mocks.removeProject,
    };
    return selector ? selector(state) : state;
  },
}));

vi.mock('@/stores/context', () => ({
  useContextStore: (selector?: (state: unknown) => unknown) => {
    const state = {
    ...mocks.contextState,
    selectContextRef: mocks.selectContextRef,
    loadProjects: vi.fn(),
    addProject: mocks.addProject,
    removeProject: mocks.removeProject,
    selectContext: vi.fn(),
    toggleProjectContext: vi.fn(),
    };
    return selector ? selector(state) : state;
  },
}));

vi.mock('@/hooks/useTauriApi', () => ({
  mapEnvironmentPath: (...args: unknown[]) => mocks.mapEnvironmentPath(...args),
  openInExplorer: (...args: unknown[]) => mocks.openInExplorer(...args),
}));

describe('ContextSidebar', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.environmentState.environments = [{
      environment: { kind: 'host' },
      displayName: 'Windows',
      status: 'available',
    }];
    mocks.environmentState.selectedEnvironment = { kind: 'host' } as EnvironmentRef;
    mocks.environmentState.projectsByEnvironment = { host: [] };
    mocks.environmentState.projectsLoaded = { host: true };
    mocks.environmentState.discoveryState = 'ready';
    mocks.environmentState.errors = {};
    mocks.contextState.selectedContext = 'global';
    mocks.contextState.selectedContextRef = {
      environment: { kind: 'host' },
      scope: { scope: 'global' },
    };
  });

  it('hides environment switching when only the host exists', () => {
    render(<ContextSidebar />);
    expect(screen.queryByRole('combobox', { name: 'context.environmentLabel' })).toBeNull();
  });

  it('shows discovered WSL distributions as flat environment options', () => {
    mocks.environmentState.environments = [
      mocks.environmentState.environments[0],
      {
        environment: { kind: 'wsl', distro_name: 'Ubuntu-24.04' },
        displayName: 'Ubuntu 24.04',
        status: 'available',
      },
      {
        environment: { kind: 'wsl', distro_name: 'Debian' },
        displayName: 'Debian',
        status: 'available',
      },
    ];

    render(<ContextSidebar />);

    const select = screen.getByRole('combobox', { name: 'context.environmentLabel' });
    expect(select).toBeDefined();
    expect(screen.getByRole('option', { name: 'Ubuntu 24.04' })).toBeDefined();
    expect(screen.getByRole('option', { name: 'Debian' })).toBeDefined();
  });

  it('prevents overlapping environment switches while WSL is connecting', () => {
    mocks.environmentState.environments = [
      mocks.environmentState.environments[0],
      {
        environment: { kind: 'wsl', distro_name: 'Ubuntu' },
        displayName: 'Ubuntu',
        status: 'connecting',
      },
    ];
    mocks.environmentState.selectedEnvironment = {
      kind: 'wsl',
      distro_name: 'Ubuntu',
    } as EnvironmentRef;
    mocks.environmentState.projectsByEnvironment = { 'wsl:Ubuntu': [] };
    mocks.environmentState.projectsLoaded = { 'wsl:Ubuntu': false };

    render(<ContextSidebar />);

    expect((screen.getByRole('combobox', {
      name: 'context.environmentLabel',
    }) as HTMLSelectElement).disabled).toBe(true);
    expect(mocks.refreshProjects).not.toHaveBeenCalled();
  });

  it('renders every project in one scrollable list', () => {
    mocks.environmentState.projectsByEnvironment = {
      host: [
        { id: 'a', nativePath: 'C:\\Code\\a', displayName: null, order: null, suppressCrossStorageWarning: false },
        { id: 'b', nativePath: 'C:\\Code\\b', displayName: null, order: null, suppressCrossStorageWarning: false },
        { id: 'c', nativePath: 'C:\\Code\\c', displayName: null, order: null, suppressCrossStorageWarning: false },
      ],
    };

    const { container } = render(<ContextSidebar />);

    expect(screen.getByText('C:\\Code\\a')).toBeDefined();
    expect(screen.getByText('C:\\Code\\b')).toBeDefined();
    expect(screen.getByText('C:\\Code\\c')).toBeDefined();
    expect(container.querySelector('[data-testid="context-sidebar-scroll"]')?.classList.contains('overflow-y-auto')).toBe(true);
  });

  it('maps a selected UNC folder before adding it to WSL', async () => {
    mocks.environmentState.environments = [
      mocks.environmentState.environments[0],
      {
        environment: { kind: 'wsl', distro_name: 'Ubuntu' },
        displayName: 'Ubuntu',
        status: 'available',
      },
    ];
    mocks.environmentState.selectedEnvironment = { kind: 'wsl', distro_name: 'Ubuntu' } as EnvironmentRef;
    mocks.environmentState.projectsByEnvironment = { 'wsl:Ubuntu': [] };
    mocks.environmentState.projectsLoaded = { 'wsl:Ubuntu': true };
    mocks.open.mockResolvedValue('\\\\wsl.localhost\\Ubuntu\\home\\me\\app');
    mocks.mapEnvironmentPath.mockResolvedValue('/home/me/app');
    mocks.addProject.mockResolvedValue([]);

    render(<ContextSidebar />);
    fireEvent.click(screen.getByRole('button', { name: 'context.addProject' }));

    await waitFor(() => expect(mocks.mapEnvironmentPath).toHaveBeenCalledWith(
      { kind: 'wsl', distro_name: 'Ubuntu' },
      '\\\\wsl.localhost\\Ubuntu\\home\\me\\app',
    ));
    expect(mocks.addProject).toHaveBeenCalledWith(
      '/home/me/app',
      { kind: 'wsl', distro_name: 'Ubuntu' },
    );
  });

  it('uses availability-scope language instead of treating global as a workspace', () => {
    expect(zhCN.context.global).toBe('全局');
    expect(zhCN.context.globalSubtitle).toBe('所有项目可用');
    expect(zhCN.context.sectionGlobal).toBe('全局');
    expect(zhCN.context.sectionProjects).toBe('项目');
  });
});
