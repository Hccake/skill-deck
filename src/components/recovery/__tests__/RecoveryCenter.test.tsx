/* @vitest-environment jsdom */

import '@/test-utils';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppError, RecoveryResourceStatus } from '@/bindings';
import { RecoveryCenter } from '../RecoveryCenter';

const mocks = vi.hoisted(() => ({
  load: vi.fn(),
  resources: [] as RecoveryResourceStatus[],
  actionStatuses: [] as RecoveryResourceStatus[],
  recoveryError: null as AppError | null,
}));

vi.mock('@/stores/recovery', () => ({
  useRecoveryStore: (selector: (state: unknown) => unknown) => selector({
    resources: mocks.resources,
    state: 'ready',
    error: mocks.recoveryError,
    load: mocks.load,
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
    subject: null,
    paths: [],
    diagnostic: null,
  };
}

describe('RecoveryCenter', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.actionStatuses = [];
    mocks.recoveryError = null;
    mocks.load.mockResolvedValue(undefined);
    mocks.resources = [];
  });

  it('does not show an entry when there are no persistent resources or load errors', () => {
    render(<RecoveryCenter />);

    expect(screen.queryByRole('button', { name: 'recovery.center.open' })).toBeNull();
  });

  it('shows a persistent resource load error without inventing another issue source', () => {
    mocks.recoveryError = {
      kind: 'io',
      data: { message: 'recovery index unavailable' },
    };

    render(<RecoveryCenter />);

    fireEvent.click(screen.getByRole('button', { name: 'recovery.center.open' }));
    expect(screen.getByText('recovery.center.loadError')).toBeDefined();
    expect(screen.getByText('recovery index unavailable')).toBeDefined();
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

  it('shows a bulk recheck only when multiple operations need attention', () => {
    mocks.resources = [resource('attention', 'needsAttention')];
    const { unmount } = render(<RecoveryCenter />);

    fireEvent.click(screen.getByRole('button', { name: 'recovery.center.open' }));
    expect(screen.queryByRole('button', { name: 'recovery.center.refreshAll' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'recovery.refresh' })).toBeNull();

    unmount();
    mocks.resources = [
      resource('attention', 'needsAttention'),
      resource('offline', 'environmentUnavailable'),
    ];
    render(<RecoveryCenter />);
    fireEvent.click(screen.getByRole('button', { name: 'recovery.center.open' }));
    expect(screen.getByRole('button', { name: 'recovery.center.refreshAll' })).toBeDefined();
  });

  it('refreshes on initial load and window focus', async () => {
    render(<RecoveryCenter />);

    await waitFor(() => expect(mocks.load).toHaveBeenCalled());
    act(() => window.dispatchEvent(new Event('focus')));
    await waitFor(() => expect(mocks.load).toHaveBeenCalledTimes(2));
  });
});
