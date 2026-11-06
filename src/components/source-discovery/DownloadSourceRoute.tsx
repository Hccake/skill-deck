import { ArrowRight } from 'lucide-react';
import type { DownloadRedirect } from '@/bindings';
import { OverflowTooltip } from '@/components/ui/overflow-tooltip';

export function DownloadSourceRoute({ originalHost, actualHost }: DownloadRedirect) {
  return (
    <span className="inline-flex min-w-0 max-w-full items-center gap-2 align-middle text-xs leading-5">
      <OverflowTooltip content={originalHost}>
        <span tabIndex={0} className="min-w-0 max-w-[48%] truncate rounded-sm text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring">{originalHost}</span>
      </OverflowTooltip>
      <ArrowRight className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
      <OverflowTooltip content={actualHost}>
        <span tabIndex={0} className="min-w-0 truncate rounded-sm font-medium text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring">{actualHost}</span>
      </OverflowTooltip>
    </span>
  );
}
