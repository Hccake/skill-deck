/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ContextRef, ProjectInfo } from '@/bindings';
import { CrossStorageWarningBanner } from '../CrossStorageWarningBanner';
import { useMutationStore } from '@/stores/mutation';

const mocks = vi.hoisted(() => ({
  context: {
    environment: { kind: 'wsl' as const, distro_name: 'Ubuntu' },
    scope: { scope: 'project' as const, project_id: 'project-1' },
  } as ContextRef,
  projects: [{
    binding: {
      id: 'project-1',
      nativePath: '/mnt/c/Code/app',
      displayName: 'app',
      order: null,
      suppressCrossStorageWarning: false,
    },
    storage: { access: 'crossStorage', owner: { kind: 'host' } },
  }] as ProjectInfo[],
  setCrossStorageWarning: vi.fn().mockResolvedValue([]),
  switchEnvironment: vi.fn().mockResolvedValue(undefined),
  transition: { kind: 'idle' } as { kind: string; target?: ContextRef['environment'] },
  environments: [
    { environment: { kind: 'host' as const }, displayName: 'Windows', status: 'available' as const },
    {
      environment: { kind: 'wsl' as const, distro_name: 'Ubuntu' },
      displayName: 'Ubuntu',
      status: 'available' as const,
    },
  ],
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: { environment?: string; owner?: string }) => {
      if (key === 'crossStorage.hostEnvironment') return 'Windows';
      if (options?.environment && options?.owner) {
        return `${key}:${options.environment}:${options.owner}`;
      }
      if (options?.owner) return `${key}:${options.owner}`;
      return key;
    },
  }),
}));

vi.mock('@/stores/workspace-context', () => ({
  useWorkspaceContextStore: (selector: (state: unknown) => unknown) => (
    selector({
      selectedContext: mocks.context,
      transition: mocks.transition,
      switchEnvironment: mocks.switchEnvironment,
    })
  ),
}));

vi.mock('@/stores/environment', () => ({
  useEnvironmentStore: (selector: (state: unknown) => unknown) => selector({
    environments: mocks.environments,
  }),
}));

vi.mock('@/hooks/useProjectWorkspace', () => ({
  useProjectWorkspace: () => ({
    projects: mocks.projects,
    setCrossStorageWarning: (projectId: string, suppressed: boolean) => (
      mocks.setCrossStorageWarning(mocks.context.environment, projectId, suppressed)
    ),
  }),
}));

describe('CrossStorageWarningBanner', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.context = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'project-1' },
    };
    mocks.projects = [{
      binding: {
        id: 'project-1',
        nativePath: '/mnt/c/Code/app',
        displayName: 'app',
        order: null,
        suppressCrossStorageWarning: false,
      },
      storage: { access: 'crossStorage', owner: { kind: 'host' } },
    }];
    mocks.transition = { kind: 'idle' };
    mocks.environments = [
      { environment: { kind: 'host' }, displayName: 'Windows', status: 'available' },
      {
        environment: { kind: 'wsl', distro_name: 'Ubuntu' },
        displayName: 'Ubuntu',
        status: 'available',
      },
    ];
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
  });

  it('names the management environment and storage owner, then persists dismissal', async () => {
    render(<CrossStorageWarningBanner />);

    expect(screen.getByText('crossStorage.title')).toBeDefined();
    expect(screen.getByText('crossStorage.description:Ubuntu:Windows')).toBeDefined();

    fireEvent.click(screen.getByRole('button', { name: 'crossStorage.dismiss' }));

    await waitFor(() => expect(mocks.setCrossStorageWarning).toHaveBeenCalledWith(
      { kind: 'wsl', distro_name: 'Ubuntu' },
      'project-1',
      true,
    ));
  });

  it('switches to the discovered storage owner Global context', () => {
    render(<CrossStorageWarningBanner />);

    fireEvent.click(screen.getByRole('button', { name: 'crossStorage.switchToOwner:Windows' }));

    expect(mocks.switchEnvironment).toHaveBeenCalledWith({ kind: 'host' });
  });

  it('does not offer a dead switch action when the owner is missing from discovery', () => {
    mocks.environments = mocks.environments.filter(
      (entry) => entry.environment.kind !== 'host',
    );

    render(<CrossStorageWarningBanner />);

    expect(screen.queryByRole('button', { name: 'crossStorage.switchToOwner:Windows' })).toBeNull();
    expect(screen.getByText('crossStorage.description:Ubuntu:Windows')).toBeDefined();
  });

  it('keeps owner switching available while a Skill mutation blocks dismissal', () => {
    useMutationStore.setState({
      activeMutation: {
        id: 'mutation-1',
        kind: 'install',
        context: { environment: { kind: 'host' }, scope: { scope: 'global' } },
        phase: 'preparing',
        progress: null,
        cancelable: true,
      },
    });

    render(<CrossStorageWarningBanner />);

    expect((screen.getByRole('button', {
      name: 'crossStorage.dismiss',
    }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', {
      name: 'crossStorage.switchToOwner:Windows',
    }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('stays hidden for native storage and previously dismissed projects', () => {
    mocks.projects[0] = {
      ...mocks.projects[0],
      storage: {
        access: 'native',
        owner: { kind: 'wsl', distro_name: 'Ubuntu' },
      },
    };
    const nativeView = render(<CrossStorageWarningBanner />);
    expect(screen.queryByText('crossStorage.title')).toBeNull();
    nativeView.unmount();

    mocks.projects[0] = {
      ...mocks.projects[0],
      binding: {
        ...mocks.projects[0].binding,
        suppressCrossStorageWarning: true,
      },
      storage: { access: 'crossStorage', owner: { kind: 'host' } },
    };
    render(<CrossStorageWarningBanner />);
    expect(screen.queryByText('crossStorage.title')).toBeNull();
  });
});
