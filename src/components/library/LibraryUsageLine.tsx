import { memo, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown, Info } from 'lucide-react';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import {
  libraryUsageDisplayName,
  libraryUsageIdentityKey,
  partitionLibraryUsages,
} from '@/lib/libraries/usage-presentation';
import { LibraryUsageIdentity } from './LibraryUsageIdentity';
import type { LibraryUsage } from '@/bindings';

interface LibraryUsageLineProps {
  usages: readonly LibraryUsage[];
}

function UsageGroup({ title, usages, tone }: {
  title: string;
  usages: readonly LibraryUsage[];
  tone?: 'warning';
}) {
  if (usages.length === 0) return null;
  return (
    <section className="space-y-1.5">
      <div className="flex items-center gap-1.5 px-1">
        {tone === 'warning' ? (
          <Info className="size-3.5 shrink-0 text-warning" aria-hidden="true" />
        ) : null}
        <p className={tone === 'warning' ? 'text-xs font-semibold text-warning' : 'text-xs font-semibold text-foreground'}>
          {title}
        </p>
      </div>
      <ul className="-mx-1 divide-y divide-border/60">
        {usages.map((usage) => (
          <li
            key={libraryUsageIdentityKey(usage)}
            className="min-w-0 px-1 py-2 first:pt-1.5 last:pb-1.5"
          >
            <LibraryUsageIdentity usage={usage} />
          </li>
        ))}
      </ul>
    </section>
  );
}

/**
 * header 元信息行中的应用状态。
 *
 * 它与其他元信息共用一行，所以宽度必须有硬上界：只有"单个位置且没有未完成调整"时直接给出
 * 位置名，其余一律折叠为计数并通过 popover 展开。侧栏统一使用计数是因为那里要纵向比较多个
 * 库；这里描述单个库、没有比较对象，最常见的单位置情况值得省掉一次点击。
 */
export const LibraryUsageLine = memo(function LibraryUsageLine({ usages }: LibraryUsageLineProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const { confirmed, pending } = useMemo(() => partitionLibraryUsages(usages), [usages]);

  if (confirmed.length === 0 && pending.length === 0) {
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <span
            data-testid="library-usage-line"
            tabIndex={0}
            className="inline-flex w-fit shrink-0 items-center rounded-sm text-xs text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
          >
            {t('libraries.usage.unapplied')}
          </span>
        </TooltipTrigger>
        <TooltipContent><p className="max-w-72">{t('libraries.unappliedHint')}</p></TooltipContent>
      </Tooltip>
    );
  }

  // 宽布局直接给出唯一位置名；紧凑布局切换为计数，并通过同一入口展开完整位置。
  if (confirmed.length === 1 && pending.length === 0) {
    const only = confirmed[0];
    const location = libraryUsageDisplayName(only, t('libraries.usage.globalLocation'));
    const summary = t('libraries.usage.appliedToLocation', { location });
    return (
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <button
            type="button"
            data-testid="library-usage-line"
            className="inline-flex min-w-0 shrink items-center gap-1 rounded-sm text-xs text-muted-foreground outline-none transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
            aria-label={t('libraries.usage.viewDetails', { summary })}
          >
            <span className="library-usage-exact min-w-0 items-center gap-1">
              <span className="shrink-0">{t('libraries.appliedTo')}</span>
              <span
                className="max-w-32 truncate font-medium text-foreground/80"
                title={location}
              >
                {location}
              </span>
            </span>
            <span className="library-usage-compact-count tabular-nums">
              {t('libraries.usage.applied', { count: 1 })}
            </span>
            <ChevronDown className="size-3 shrink-0" aria-hidden="true" />
          </button>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-72 space-y-4">
          <UsageGroup title={t('libraries.appliedTo')} usages={confirmed} />
        </PopoverContent>
      </Popover>
    );
  }

  const visibleSummary = confirmed.length > 0
    ? t('libraries.usage.applied', { count: confirmed.length })
    : t('libraries.usage.pendingOnly');
  const accessibleSummary = confirmed.length > 0 && pending.length > 0
    ? t('libraries.usage.appliedWithPending', { count: confirmed.length })
    : visibleSummary;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          data-testid="library-usage-line"
          className="inline-flex shrink-0 items-center gap-1 rounded-sm text-xs text-muted-foreground outline-none transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
          aria-label={t('libraries.usage.viewDetails', { summary: accessibleSummary })}
        >
          <span className="tabular-nums">{visibleSummary}</span>
          {pending.length > 0 ? (
            <Info className="size-3.5 shrink-0 text-warning" aria-hidden="true" />
          ) : null}
          <ChevronDown className="size-3 shrink-0" aria-hidden="true" />
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-72 space-y-4">
        <UsageGroup title={t('libraries.appliedTo')} usages={confirmed} />
        <UsageGroup title={t('libraries.pendingAdjustment')} usages={pending} tone="warning" />
        {pending.length > 0 ? (
          <p className="text-xs text-muted-foreground">
            {t('libraries.pendingAdjustmentDescription')}
          </p>
        ) : null}
      </PopoverContent>
    </Popover>
  );
});
