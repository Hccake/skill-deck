/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ContextRef, ProjectBinding } from '@/bindings';
import { CrossStorageWarningBanner } from '../CrossStorageWarningBanner';
import { useMutationStore } from '@/stores/mutation';

const mocks = vi.hoisted(() => ({
  context: {
    environment: { kind: 'wsl' as const, distro_name: 'Ubuntu' },
    scope: { scope: 'project' as const, project_id: 'project-1' },
  } as ContextRef,
  projects: [{
    id: 'project-1',
    nativePath: '/mnt/c/Code/app',
    displayName: 'app',
    order: null,
    suppressCrossStorageWarning: false,
  }] as ProjectBinding[],
  suppressCrossStorageWarning: vi.fn().mockResolvedValue([]),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@/stores/context', () => ({
  useContextStore: (selector: (state: { selectedContextRef: ContextRef }) => unknown) => (
    selector({ selectedContextRef: mocks.context })
  ),
}));

vi.mock('@/stores/environment', () => ({
  environmentKey: (environment: { kind: string; distro_name?: string }) => (
    environment.kind === 'host' ? 'host' : `wsl:${environment.distro_name}`
  ),
  useEnvironmentStore: (selector: (state: unknown) => unknown) => selector({
    projectsByEnvironment: { 'wsl:Ubuntu': mocks.projects },
    suppressCrossStorageWarning: mocks.suppressCrossStorageWarning,
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
      id: 'project-1',
      nativePath: '/mnt/c/Code/app',
      displayName: 'app',
      order: null,
      suppressCrossStorageWarning: false,
    }];
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
  });

  it('shows a non-blocking warning and persists dismissal for a cross-storage project', async () => {
    render(<CrossStorageWarningBanner />);

    expect(screen.getByText('crossStorage.title')).toBeDefined();
    expect(screen.getByText('crossStorage.description')).toBeDefined();

    fireEvent.click(screen.getByRole('button', { name: 'crossStorage.dismiss' }));

    await waitFor(() => expect(mocks.suppressCrossStorageWarning).toHaveBeenCalledWith(
      'project-1',
      { kind: 'wsl', distro_name: 'Ubuntu' },
    ));
  });

  it('stays hidden for native storage and previously dismissed projects', () => {
    mocks.projects[0] = {
      ...mocks.projects[0],
      nativePath: '/home/alice/app',
    };
    const nativeView = render(<CrossStorageWarningBanner />);
    expect(screen.queryByText('crossStorage.title')).toBeNull();
    nativeView.unmount();

    mocks.projects[0] = {
      ...mocks.projects[0],
      nativePath: '/mnt/c/Code/app',
      suppressCrossStorageWarning: true,
    };
    render(<CrossStorageWarningBanner />);
    expect(screen.queryByText('crossStorage.title')).toBeNull();
  });
});
