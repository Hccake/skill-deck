import { Folder, Globe2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { libraryUsageDisplayName } from '@/lib/libraries/usage-presentation';
import type { LibraryUsage } from '@/bindings';
import { OverflowTooltip } from '@/components/ui/overflow-tooltip';

interface LibraryUsageIdentityProps {
  usage: LibraryUsage;
  showPath?: boolean;
}

export function LibraryUsageIdentity({
  usage,
  showPath = true,
}: LibraryUsageIdentityProps) {
  const { t } = useTranslation();
  const projectUsage = usage.context.scope.scope === 'project';
  const name = libraryUsageDisplayName(usage, t('libraries.usage.globalLocation'));
  const path = projectUsage ? usage.project?.nativePath ?? null : null;
  const Icon = projectUsage ? Folder : Globe2;

  return (
    <span className="inline-flex min-w-0 items-start gap-2 text-left">
      <Icon className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
      <span className="min-w-0 flex-1">
        <OverflowTooltip content={name}><span tabIndex={0} className="block truncate text-xs font-medium text-foreground">
          {name}
        </span></OverflowTooltip>
        {showPath && path ? (
          <OverflowTooltip content={path}><span tabIndex={0}
            data-testid="library-usage-path"
            className="mt-0.5 block truncate font-mono text-[10px] text-muted-foreground"
            translate="no"
          >
            {path}
          </span></OverflowTooltip>
        ) : null}
      </span>
    </span>
  );
}
