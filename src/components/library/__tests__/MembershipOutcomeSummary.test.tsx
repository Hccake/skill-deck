/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { MembershipOutcomeSummary } from '../MembershipOutcomeSummary';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) => (
      values ? `${key}:${JSON.stringify(values)}` : key
    ),
  }),
}));

describe('MembershipOutcomeSummary', () => {
  it('renders nothing when there is no result or action to display', () => {
    const { container } = render(<MembershipOutcomeSummary outcome={{
      scopes: [], cleanup: [], snapshotError: null,
    }} />);

    expect(container.childElementCount).toBe(0);
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('keeps every Scope and cleanup state distinct', () => {
    const context = { environment: { kind: 'native' as const }, scope: { scope: 'global' as const } };
    render(<MembershipOutcomeSummary outcome={{
      scopes: [
        { context, state: 'synced', error: null },
        { context, state: 'pending', error: null },
        { context, state: 'unverified', error: null },
        { context, state: 'recoveryRequired', error: null },
        { context, state: 'cancelled', error: null },
      ],
      cleanup: [
        { libraryId: 'lib-1', memberName: 'a', retirementId: '1', state: 'purged', error: null },
        { libraryId: 'lib-1', memberName: 'b', retirementId: '2', state: 'retained', error: null },
        { libraryId: 'lib-1', memberName: 'c', retirementId: '3', state: 'failed', error: null },
      ],
      snapshotError: { kind: 'staleTarget' },
    }} />);

    expect(screen.getByText('libraries.membership.scopeResult.synced:{"count":1}')).toBeTruthy();
    expect(screen.getByText('libraries.membership.scopeResult.pending:{"count":1}')).toBeTruthy();
    expect(screen.getByText('libraries.membership.scopeResult.unverified:{"count":1}')).toBeTruthy();
    expect(screen.getByText('libraries.membership.scopeResult.recoveryRequired:{"count":1}')).toBeTruthy();
    expect(screen.getByText('libraries.membership.scopeResult.cancelled:{"count":1}')).toBeTruthy();
    expect(screen.getByText('libraries.membership.cleanupState.purged:{"count":1}')).toBeTruthy();
    expect(screen.getByText('libraries.membership.cleanupState.retained:{"count":1}')).toBeTruthy();
    expect(screen.getByText('libraries.membership.cleanupState.failed:{"count":1}')).toBeTruthy();
    expect(screen.getByText('libraries.membership.snapshotError')).toBeTruthy();
  });
});
