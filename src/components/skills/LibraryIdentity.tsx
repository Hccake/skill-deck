import { LibraryBig } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { SkillLibrarySummary } from '@/bindings';

export function LibraryIdentity({ library }: { library: SkillLibrarySummary }) {
  const { t } = useTranslation();
  return (
    <>
      <span
        data-testid="library-icon"
        className="flex shrink-0 items-center justify-center text-emerald-600 dark:text-emerald-400"
        aria-hidden="true"
      >
        <LibraryBig className="size-3.5 translate-y-[0.5px]" />
      </span>
      <span data-testid="library-summary-line" className="flex min-w-0 items-center gap-1.5 leading-none">
        <span className="min-w-0 truncate text-xs font-semibold leading-none text-foreground/90">{library.name}</span>
        <span aria-hidden="true" className="shrink-0 text-muted-foreground/40 leading-none select-none">·</span>
        <span className="shrink-0 whitespace-nowrap text-xs tabular-nums leading-none text-muted-foreground">
          {t('libraries.skillCount', { count: library.skillCount })}
        </span>
      </span>
    </>
  );
}
