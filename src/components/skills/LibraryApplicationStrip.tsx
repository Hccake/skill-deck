import { useTranslation } from 'react-i18next';
import type { LibraryApplicationSummary } from '@/bindings';
import { cn } from '@/lib/utils';
import { LibraryIdentity } from './LibraryIdentity';

interface LibraryApplicationStripProps {
  application: LibraryApplicationSummary;
  compact?: boolean;
}

function LibrarySummaryItem({
  library,
  compact = false,
}: {
  library: LibraryApplicationSummary['orderedLibraries'][number];
  compact?: boolean;
}) {
  return (
    <div
      data-testid="library-summary-item"
      className={cn(
        'inline-flex min-w-0 items-center gap-1.5 rounded-lg border border-border/50 bg-muted/50 px-2.5 text-left transition-colors hover:bg-muted/80 hover:border-border/80',
        compact ? 'h-7.5 w-full' : 'h-7.5 max-w-64 shrink-0',
      )}
      title={library.name}
    >
      <LibraryIdentity library={library} />
    </div>
  );
}

export function LibraryApplicationStrip({ application, compact = false }: LibraryApplicationStripProps) {
  const { t } = useTranslation();
  if (application.orderedLibraries.length === 0 && !application.pending) return null;

  return (
    <div
      className={cn(
        'flex min-w-0 gap-2',
        compact ? 'mb-2 flex-col px-1.5' : 'mb-2.5 flex-wrap items-center',
      )}
      data-testid="applied-libraries-summary"
    >
      {application.orderedLibraries.length > 0 ? (
        <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2">
          {application.orderedLibraries.map((library) => (
            <LibrarySummaryItem key={library.id} library={library} compact={compact} />
          ))}
        </div>
      ) : null}
      {application.syncState !== 'synced' ? (
        <span role="status" className="shrink-0 text-xs font-medium text-warning">
          {t(`libraries.syncState.${application.syncState}`)}
        </span>
      ) : null}
    </div>
  );
}
