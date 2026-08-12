/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MutationStatusBar } from '../MutationStatusBar';
import type { ActiveMutation, EnvironmentInfo, ProjectInfo } from '@/bindings';

const mutation: ActiveMutation = {
  kind: 'update',
  context: {
    environment: { kind: 'wsl', distro_name: 'Ubuntu' },
    scope: { scope: 'project', project_id: 'project-1' },
  },
  id: 'mutation-1',
  phase: 'preparing',
  progress: null,
  cancelable: true,
};

const mocks = vi.hoisted(() => ({
  cancelActiveMutation: vi.fn().mockResolvedValue(true),
  mutationState: {
    activeMutation: null as ActiveMutation | null,
    cancelling: false,
  },
  environmentState: {
    environments: [] as EnvironmentInfo[],
  },
  projectState: {
    projectsByEnvironment: {} as Record<string, ProjectInfo[]>,
  },
  installWizardState: {
    active: false,
    loading: false,
    syncError: null as 'monitor' | 'refresh' | null,
  },
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) => {
      if (key === 'mutation.status') {
        return `${values?.environment} / ${values?.scope} - ${values?.status}`;
      }
      return key;
    },
  }),
}));

vi.mock('@/stores/mutation', () => ({
  useMutationStore: (selector: (state: unknown) => unknown) => selector({
    ...mocks.mutationState,
    cancelActiveMutation: mocks.cancelActiveMutation,
  }),
}));

vi.mock('@/hooks/useMutationMonitor', () => ({
  useMutationMonitor: vi.fn(),
}));

vi.mock('@/stores/environment', () => ({
  environmentKey: (environment: { kind: string; distro_name?: string }) => (
    environment.kind === 'native' ? 'native' : `wsl:${environment.distro_name?.toLowerCase()}`
  ),
  useEnvironmentStore: (selector: (state: unknown) => unknown) => selector(mocks.environmentState),
}));

vi.mock('@/hooks/useProjectWorkspace', () => ({
  useProjectWorkspace: (environment: { kind: string; distro_name?: string }) => {
    const key = environment.kind === 'native' ? 'native' : `wsl:${environment.distro_name?.toLowerCase()}`;
    return { projects: mocks.projectState.projectsByEnvironment[key] ?? [] };
  },
}));

vi.mock('@/stores/install-wizard-session', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/stores/install-wizard-session')>(),
  useInstallWizardSessionStore: (selector: (state: unknown) => unknown) => (
    selector(mocks.installWizardState)
  ),
}));

describe('MutationStatusBar', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.mutationState.activeMutation = null;
    mocks.mutationState.cancelling = false;
    mocks.environmentState.environments = [];
    mocks.projectState.projectsByEnvironment = {};
    mocks.installWizardState.active = false;
    mocks.installWizardState.loading = false;
    mocks.installWizardState.syncError = null;
  });

  it('shows the mutation environment, project, status, and cancel action', async () => {
    mocks.mutationState.activeMutation = mutation;
    mocks.environmentState.environments = [{
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      displayName: 'Ubuntu 24.04',
      status: 'available',
      revision: 1,
      error: null,
    }];
    mocks.projectState.projectsByEnvironment = {
      'wsl:ubuntu': [{
        binding: {
          id: 'project-1',
          nativePath: '/home/user/cgp-be',
          displayName: 'cgp-be',
          order: null,
          suppressCrossStorageWarning: false,
        },
        storage: { access: 'native', owner: mutation.context.environment },
      }],
    };

    render(<MutationStatusBar />);

    expect(screen.getByText('context.environmentWslName / cgp-be - mutation.activity')).toBeDefined();
    expect(screen.getByTestId('mutation-spinner')).toBeDefined();
    fireEvent.click(screen.getByRole('button', { name: 'mutation.cancel' }));

    await waitFor(() => expect(mocks.cancelActiveMutation).toHaveBeenCalledTimes(1));
  });

  it('shows cancellation progress and hides the cancel action for non-cancelable work', () => {
    mocks.mutationState.activeMutation = { ...mutation, cancelable: false };
    mocks.mutationState.cancelling = true;

    render(<MutationStatusBar />);

    expect(screen.getByText('mutation.cancelling')).toBeDefined();
    expect(screen.queryByRole('button', { name: 'mutation.cancel' })).toBeNull();
  });

  it('does not let the main window cancel work owned by the install wizard', () => {
    mocks.mutationState.activeMutation = mutation;
    mocks.installWizardState.active = true;

    render(<MutationStatusBar />);

    const cancelButton = screen.getByRole('button', { name: 'mutation.cancel' });
    expect((cancelButton as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(cancelButton);
    expect(mocks.cancelActiveMutation).not.toHaveBeenCalled();
  });
});
