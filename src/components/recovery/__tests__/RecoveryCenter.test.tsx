/* @vitest-environment jsdom */

import '@/test-utils';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppError, RecoveryResourceStatus } from '@/bindings';
import { RecoveryCenter } from '../RecoveryCenter';

const mocks = vi.hoisted(() => ({
  load: vi.fn(),
  resources: [] as RecoveryResourceStatus[],
  maintenance: [] as Array<{ environment: { kind: 'host' } | { kind: 'wsl'; distro_name: string }; state: 'pending' | 'ready' | 'failed'; issues: string[] }>,
  actionStatuses: [] as RecoveryResourceStatus[],
  recoveryError: null as AppError | null,
  discoveryError: null as AppError | null,
  errorsByEnvironment: {} as Record<string, AppError | null>,
  listen: vi.fn().mockResolvedValue(() => undefined),
}));

vi.mock('@/bindings', () => ({
  events: {
    runtimeMaintenanceChanged: {
      listen: mocks.listen,
    },
  },
}));

vi.mock('@/stores/recovery', () => ({
  useRecoveryStore: (selector: (state: unknown) => unknown) => selector({
    resources: mocks.resources,
    maintenance: mocks.maintenance,
    state: 'ready',
    error: mocks.recoveryError,
    load: mocks.load,
    applyMaintenance: vi.fn(),
  }),
}));

vi.mock('@/stores/environment', () => ({
  useEnvironmentStore: (selector: (state: unknown) => unknown) => selector({
    environments: [{
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      displayName: 'Ubuntu',
      status: 'unavailable',
      revision: 2,
      error: null,
    }],
    discoveryState: mocks.discoveryError ? 'error' : 'ready',
    discoveryError: mocks.discoveryError,
    errorsByEnvironment: mocks.errorsByEnvironment,
  }),
}));

vi.mock('@/components/recovery/RecoveryActions', () => ({
  RecoveryActions: ({
    recovery,
    initialStatus,
  }: {
    recovery: { resourceId: string };
    initialStatus?: RecoveryResourceStatus;
  }) => {
    if (initialStatus) mocks.actionStatuses.push(initialStatus);
    return <div>actions:{recovery.resourceId}</div>;
  },
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: { count?: number; environment?: string }) => {
      const value = values?.count ?? values?.environment;
      return value === undefined ? key : `${key}:${value}`;
    },
  }),
}));

function resource(
  resourceId: string,
  state: RecoveryResourceStatus['state'],
): RecoveryResourceStatus {
  return {
    resourceId,
    state,
    revision: `revision-${resourceId}`,
    environment: { kind: 'wsl', distro_name: 'Ubuntu' },
    createdAtEpochMs: 1,
    displayPaths: [],
    diagnostic: null,
  };
}

describe('RecoveryCenter', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.actionStatuses = [];
    mocks.maintenance = [];
    mocks.recoveryError = null;
    mocks.discoveryError = null;
    mocks.errorsByEnvironment = {};
    mocks.load.mockResolvedValue(undefined);
    mocks.resources = [];
  });

  it('does not show an entry for normal or pending maintenance', () => {
    mocks.maintenance = [
      { environment: { kind: 'host' }, state: 'ready', issues: [] },
      { environment: { kind: 'wsl', distro_name: 'Ubuntu' }, state: 'pending', issues: [] },
    ];

    render(<RecoveryCenter />);

    expect(screen.queryByRole('button', { name: 'recovery.center.open' })).toBeNull();
  });

  it('shows failed maintenance in a stable dialog without retry', () => {
    mocks.maintenance = [{
      environment: { kind: 'host' },
      state: 'failed',
      issues: ['payloadSweepFailed'],
    }];

    render(<RecoveryCenter />);

    fireEvent.click(screen.getByRole('button', { name: 'recovery.center.open' }));
    expect(screen.getByRole('dialog')).toBeDefined();
    expect(screen.getByText('recovery.maintenance.title:mutation.host')).toBeDefined();
    expect(screen.getByText('recovery.maintenance.failed')).toBeDefined();
    expect(screen.getByText('recovery.maintenance.issues.payloadSweepFailed')).toBeDefined();
    expect(screen.queryByText('recovery.retryMaintenance')).toBeNull();
  });

  it('shows saved Discovery and Connection errors in the dialog', () => {
    mocks.discoveryError = {
      kind: 'environmentDiscoveryFailed',
      data: { message: 'wsl.exe timed out' },
    };
    mocks.errorsByEnvironment = {
      'wsl:ubuntu': {
        kind: 'environmentUnavailable',
        data: {
          environment: { kind: 'wsl', distro_name: 'Ubuntu' },
          message: 'distribution is unavailable',
        },
      },
    };

    render(<RecoveryCenter />);

    fireEvent.click(screen.getByRole('button', { name: 'recovery.center.open' }));
    expect(screen.getByText('recovery.environment.discoveryTitle')).toBeDefined();
    expect(screen.getByText('recovery.environment.connectionTitle:Ubuntu')).toBeDefined();
    expect(screen.queryByText('context.environmentRetry')).toBeNull();
  });

  it('keeps recovery resource actions available in the dialog', () => {
    mocks.resources = [
      resource('attention', 'needsAttention'),
      resource('offline', 'environmentUnavailable'),
    ];

    render(<RecoveryCenter />);

    fireEvent.click(screen.getByRole('button', { name: 'recovery.center.open' }));
    expect(screen.getByText('actions:attention')).toBeDefined();
    expect(screen.getByText('actions:offline')).toBeDefined();
    expect(mocks.actionStatuses).toEqual([
      expect.objectContaining({ resourceId: 'attention', state: 'needsAttention' }),
      expect.objectContaining({ resourceId: 'offline', state: 'environmentUnavailable' }),
    ]);
  });

  it('keeps initial and focus recovery refresh without registering a second Environment listener', async () => {
    render(<RecoveryCenter />);

    await waitFor(() => expect(mocks.load).toHaveBeenCalled());
    act(() => window.dispatchEvent(new Event('focus')));
    await waitFor(() => expect(mocks.load).toHaveBeenCalledTimes(2));
  });
});
