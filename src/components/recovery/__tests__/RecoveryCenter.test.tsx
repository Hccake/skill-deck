/* @vitest-environment jsdom */

import '@/test-utils';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { RecoveryResourceStatus } from '@/bindings';
import { RecoveryCenter } from '../RecoveryCenter';

const mocks = vi.hoisted(() => ({
  load: vi.fn(),
  resources: [] as RecoveryResourceStatus[],
  maintenance: [] as Array<{ environment: { kind: 'host' } | { kind: 'wsl'; distro_name: string }; state: 'pending' | 'ready' | 'failed'; issues: string[] }>,
  actionStatuses: [] as RecoveryResourceStatus[],
}));

vi.mock('@/stores/recovery', () => ({
  useRecoveryStore: (selector: (state: unknown) => unknown) => selector({
    resources: mocks.resources,
    maintenance: mocks.maintenance,
    state: 'ready',
    error: null,
    load: mocks.load,
    applyMaintenance: vi.fn(),
    retryMaintenance: vi.fn().mockResolvedValue(undefined),
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
  useTranslation: () => ({ t: (key: string, values?: { count?: number }) => (
    values?.count === undefined ? key : `${key}:${values.count}`
  ) }),
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
    mocks.load.mockResolvedValue(undefined);
    mocks.resources = [
      resource('attention', 'needsAttention'),
      resource('offline', 'environmentUnavailable'),
    ];
  });

  it('surfaces restart recovery globally and keeps unavailable environments refresh-only', () => {
    render(<RecoveryCenter />);

    expect(screen.getByText('recovery.center.count:2')).toBeDefined();
    fireEvent.click(screen.getByRole('button', { name: 'recovery.center.show' }));
    expect(screen.getByText('actions:attention')).toBeDefined();
    expect(mocks.actionStatuses).toEqual([
      expect.objectContaining({ resourceId: 'attention', state: 'needsAttention' }),
    ]);
    expect(screen.getByText('recovery.state.environmentUnavailable')).toBeDefined();
    expect(screen.queryByText('actions:offline')).toBeNull();
    fireEvent.click(screen.getAllByRole('button', { name: 'recovery.refresh' })[1]);
    expect(mocks.load).toHaveBeenCalled();
  });

  it('keeps initial and focus recovery refresh without registering a second Environment listener', async () => {
    render(<RecoveryCenter />);

    await waitFor(() => expect(mocks.load).toHaveBeenCalled());
    act(() => window.dispatchEvent(new Event('focus')));
    await waitFor(() => expect(mocks.load).toHaveBeenCalledTimes(2));
  });

  it('keeps maintenance retry reachable when no recovery resource exists', () => {
    mocks.resources = [];
    mocks.maintenance = [{
      environment: { kind: 'host' },
      state: 'failed',
      issues: ['payloadSweepFailed'],
    }];

    render(<RecoveryCenter />);

    fireEvent.click(screen.getByRole('button', { name: 'recovery.center.show' }));
    expect(screen.getByRole('button', { name: 'recovery.retryMaintenance' })).toBeDefined();
  });
});
