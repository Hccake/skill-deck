import type { LibraryMembershipOutcome } from '@/bindings';

export function membershipNeedsAttention(outcome: LibraryMembershipOutcome | undefined): boolean {
  return Boolean(outcome && (
    outcome.snapshotError
    || outcome.scopes.some((scope) => scope.state !== 'synced')
    || outcome.cleanup.some((item) => item.state === 'failed')
  ));
}
