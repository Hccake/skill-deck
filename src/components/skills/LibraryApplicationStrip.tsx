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
        'grid min-w-0 grid-cols-[1.5rem_minmax(0,1fr)] items-center gap-1.5 rounded-md border border-primary/15 bg-primary/[0.04] px-2.5 text-left',
        compact ? 'h-10 w-full' : 'h-10 min-w-40 max-w-64 shrink-0',
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
        compact ? 'mb-2 flex-col px-1.5' : 'mb-3 flex-wrap items-center',
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
      {application.pending ? <span role="status" className="shrink-0 text-xs font-medium text-warning">{t('libraries.pending')}</span> : null}
    </div>
  );
}
