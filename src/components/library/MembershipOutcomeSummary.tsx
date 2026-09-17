import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type {
  LibraryMembershipOutcome,
  MembershipScopeState,
  RetiredCleanupState,
} from '@/bindings';

const SCOPE_STATES: MembershipScopeState[] = [
  'synced',
  'pending',
  'unverified',
  'recoveryRequired',
  'cancelled',
];
const CLEANUP_STATES: RetiredCleanupState[] = ['purged', 'retained', 'failed'];

export function MembershipOutcomeSummary({
  outcome,
  action,
}: {
  outcome: LibraryMembershipOutcome;
  action?: ReactNode;
}) {
  const { t } = useTranslation();
  if (outcome.scopes.length === 0 && outcome.cleanup.length === 0 && !outcome.snapshotError && !action) {
    return null;
  }
  return (
    <div className="flex items-start justify-between gap-3" role="status">
      <div className="flex flex-wrap gap-x-3 gap-y-1 text-xs text-muted-foreground">
        {SCOPE_STATES.map((state) => {
          const count = outcome.scopes.filter((scope) => scope.state === state).length;
          return count > 0 ? (
            <span key={state}>{t(`libraries.membership.scopeResult.${state}`, { count })}</span>
          ) : null;
        })}
        {CLEANUP_STATES.map((state) => {
          const count = outcome.cleanup.filter((item) => item.state === state).length;
          return count > 0 ? (
            <span key={state}>{t(`libraries.membership.cleanupState.${state}`, { count })}</span>
          ) : null;
        })}
        {outcome.snapshotError ? (
          <span className="text-warning">{t('libraries.membership.snapshotError')}</span>
        ) : null}
      </div>
      {action}
    </div>
  );
}
