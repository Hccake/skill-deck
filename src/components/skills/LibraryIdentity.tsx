import { LibraryBig } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { SkillLibrarySummary } from '@/bindings';

export function LibraryIdentity({ library }: { library: SkillLibrarySummary }) {
  const { t } = useTranslation();
  return (
    <>
      <span
        data-testid="library-icon"
        className="grid size-6 place-items-center rounded bg-muted/40 text-muted-foreground"
        aria-hidden="true"
      >
        <LibraryBig className="size-3.5" />
      </span>
      <span data-testid="library-summary-line" className="flex min-w-0 items-center gap-1 pl-0.5">
        <span className="min-w-0 truncate text-sm font-semibold text-foreground">{library.name}</span>
        <span aria-hidden="true" className="shrink-0 text-border">·</span>
        <span className="shrink-0 whitespace-nowrap text-xs tabular-nums text-muted-foreground">
          {t('libraries.skillCount', { count: library.skillCount })}
        </span>
      </span>
    </>
  );
}
