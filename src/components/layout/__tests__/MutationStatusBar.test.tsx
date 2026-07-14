/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MutationStatusBar } from '../MutationStatusBar';
import type { ActiveMutation, EnvironmentInfo, ProjectBinding } from '@/bindings';

const mutation: ActiveMutation = {
  kind: 'update',
  context: {
    environment: { kind: 'wsl', distro_name: 'Ubuntu' },
    scope: { scope: 'project', project_id: 'project-1' },
  },
  statusText: 'Updating toolkit',
  cancelable: true,
};

const mocks = vi.hoisted(() => ({
  refreshMutation: vi.fn().mockResolvedValue(undefined),
  cancelActiveMutation: vi.fn().mockResolvedValue(true),
  mutationState: {
    activeMutation: null as ActiveMutation | null,
    cancelling: false,
  },
  environmentState: {
    environments: [] as EnvironmentInfo[],
    projectsByEnvironment: {} as Record<string, ProjectBinding[]>,
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
    refreshMutation: mocks.refreshMutation,
    cancelActiveMutation: mocks.cancelActiveMutation,
  }),
}));

vi.mock('@/stores/environment', () => ({
  environmentKey: (environment: { kind: string; distro_name?: string }) => (
    environment.kind === 'host' ? 'host' : `wsl:${environment.distro_name}`
  ),
  useEnvironmentStore: (selector: (state: unknown) => unknown) => selector(mocks.environmentState),
}));

describe('MutationStatusBar', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.mutationState.activeMutation = null;
    mocks.mutationState.cancelling = false;
    mocks.environmentState.environments = [];
    mocks.environmentState.projectsByEnvironment = {};
  });

  it('refreshes immediately, on focus, and while the main window stays open', async () => {
    vi.useFakeTimers();

    render(<MutationStatusBar pollIntervalMs={2_000} />);
    await vi.waitFor(() => expect(mocks.refreshMutation).toHaveBeenCalledTimes(1));

    window.dispatchEvent(new Event('focus'));
    await vi.waitFor(() => expect(mocks.refreshMutation).toHaveBeenCalledTimes(2));

    await vi.advanceTimersByTimeAsync(2_000);
    expect(mocks.refreshMutation).toHaveBeenCalledTimes(3);

    vi.useRealTimers();
  });

  it('shows the mutation environment, project, status, and cancel action', async () => {
    mocks.mutationState.activeMutation = mutation;
    mocks.environmentState.environments = [{
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      displayName: 'Ubuntu 24.04',
      status: 'available',
    }];
    mocks.environmentState.projectsByEnvironment = {
      'wsl:Ubuntu': [{
        id: 'project-1',
        nativePath: '/home/user/cgp-be',
        displayName: 'cgp-be',
        order: null,
        suppressCrossStorageWarning: false,
      }],
    };

    render(<MutationStatusBar />);

    expect(screen.getByText('Ubuntu 24.04 / cgp-be - Updating toolkit')).toBeDefined();
    const spinner = screen.getByTestId('mutation-spinner');
    expect(spinner.className).toContain('animate-spin');
    expect(spinner.querySelector('svg')?.className.baseVal).not.toContain('animate-spin');
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
});
