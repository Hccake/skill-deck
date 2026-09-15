import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, Copy } from 'lucide-react';
import { toast } from 'sonner';
import type { ResourceLocator, ScopePathBase, SkillLocation } from '@/bindings';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { presentInstallPath } from '@/lib/install-path';

export function InstallPath({ path, base, scope, copyable = true }: {
  path: ResourceLocator;
  base?: ScopePathBase | null;
  scope: SkillLocation['scope'];
  copyable?: boolean;
}) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const { label } = presentInstallPath(path, base, scope);
  const separator = base?.pathStyle === 'windows' ? '\\' : '/';
  const split = label.lastIndexOf(separator) + 1;

  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(timer);
  }, [copied]);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(path.nativePath);
      setCopied(true);
    } catch {
      toast.error(t('skills.installPath.copyFailed'));
    }
  };

  return (
    <TooltipProvider>
    <div className="flex min-w-0 max-w-full items-center gap-1">
      <Tooltip>
        <TooltipTrigger asChild>
          <button
            type="button"
            aria-label={t('skills.installPath.viewFullPath', { path: label })}
            className="min-w-0 max-w-full rounded-sm text-left text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
          >
            <code className="flex min-w-0 max-w-full text-xs leading-5" translate="no" aria-hidden="true">
              {split > 0 ? <span className="min-w-0 truncate">{label.slice(0, split)}</span> : null}
              <span className="max-w-full shrink-0 truncate">{label.slice(split)}</span>
            </code>
          </button>
        </TooltipTrigger>
        <TooltipContent className="max-w-[min(32rem,calc(100vw-3rem))] break-all text-wrap">
          <code translate="no">{path.nativePath}</code>
        </TooltipContent>
      </Tooltip>
      {copyable ? <Tooltip>
        <TooltipTrigger asChild>
            <Button variant="ghost" size="icon" className="size-7 shrink-0 text-muted-foreground hover:text-foreground"
              aria-label={t('skills.installPath.copyPath')} onClick={() => { void copy(); }}>
              {copied ? <Check className="size-3.5" aria-hidden="true" /> : <Copy className="size-3.5" aria-hidden="true" />}
            </Button>
        </TooltipTrigger>
        <TooltipContent>{t('skills.installPath.copyPath')}</TooltipContent>
      </Tooltip> : null}
      <span role="status" className="sr-only">{copied ? t('skills.installPath.copied') : ''}</span>
    </div>
    </TooltipProvider>
  );
}
