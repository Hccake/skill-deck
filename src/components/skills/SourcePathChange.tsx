import { useTranslation } from 'react-i18next';
import type { SourcePathChange as SourcePathChangeInfo } from '@/bindings';
import { OverflowTooltip } from '@/components/ui/overflow-tooltip';

export function SourcePathChange({ change }: { change: SourcePathChangeInfo }) {
  const { t } = useTranslation();
  return (
    <span className="flex min-w-[min(22.5rem,100%)] max-w-full flex-[1_1_22.5rem] items-baseline gap-1.5 text-xs font-normal text-muted-foreground">
      <span className="shrink-0">{t('skills.updatePlan.pathChange')}</span>
      <OverflowTooltip content={<code translate="no">{change.previousPath}</code>}>
        <code tabIndex={0} translate="no" data-tooltip-label className="min-w-0 truncate rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
          {change.previousPath}
        </code>
      </OverflowTooltip>
      <span className="shrink-0">→</span>
      <OverflowTooltip content={<code translate="no">{change.nextPath}</code>}>
        <code tabIndex={0} translate="no" data-tooltip-label className="min-w-0 truncate rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
          {change.nextPath}
        </code>
      </OverflowTooltip>
    </span>
  );
}
