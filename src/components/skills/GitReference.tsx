import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, Copy, GitBranch, GitCommitHorizontal, Tag } from 'lucide-react';
import { toast } from 'sonner';
import type { GitRefKind } from '@/bindings';
import { OverflowTooltip } from '@/components/ui/overflow-tooltip';
import { DetailAction, DetailField } from './detail/DetailPrimitives';

interface GitReferenceProps {
  value: string;
  kind?: GitRefKind | null;
}

export function GitReference({ value, kind }: GitReferenceProps) {
  const { t } = useTranslation();
  const Icon = kind === 'commit' ? GitCommitHorizontal : kind === 'tag' ? Tag : kind === 'branch' ? GitBranch : null;
  const label = t(`skills.reference.${kind ?? 'unknown'}`);
  return (
    <OverflowTooltip content={<span>{label} <code>{value}</code></span>} always={kind === 'commit'}>
      <span tabIndex={0} className="inline-flex min-w-0 max-w-full items-center gap-1 rounded-sm text-xs text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring">
        {Icon ? <Icon className="size-3 shrink-0" aria-hidden="true" /> : null}
        <span className="shrink-0">{label}</span>
        <code className="min-w-0 truncate font-mono">{kind === 'commit' ? value.slice(0, 8) : value}</code>
      </span>
    </OverflowTooltip>
  );
}

export function GitReferenceField({ value, kind }: GitReferenceProps) {
  const { t } = useTranslation();
  const [copiedValue, setCopiedValue] = useState<string | null>(null);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopiedValue(value);
    } catch {
      toast.error(t('skills.reference.copyFailed'));
    }
  };
  return (
    <DetailField label={t(`skills.reference.${kind ?? 'unknown'}`)}>
      <span className="flex min-w-0 items-start gap-1">
        <code className="min-w-0 break-all font-mono">{value}</code>
        <DetailAction label={t('skills.reference.copy')} className="size-5" onClick={() => void copy()} onBlur={() => setCopiedValue(null)}>
          {copiedValue === value ? <Check className="size-3" aria-hidden="true" /> : <Copy className="size-3" aria-hidden="true" />}
        </DetailAction>
      </span>
    </DetailField>
  );
}
