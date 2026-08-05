import { CircleHelp, Copy, Link2 } from 'lucide-react';
import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import type { AgentSelectionSnapshot, InstallMode } from '@/bindings';
import { Label } from '@/components/ui/label';
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import {
  shouldShowInstallMode,
  type AgentSelectionSession,
} from '@/lib/agent-selection-session';

interface AgentSelectionModeControlProps {
  snapshot: AgentSelectionSnapshot;
  session: AgentSelectionSession;
  onModeChange: (mode: InstallMode) => void;
  disabled?: boolean;
  className?: string;
}

export function AgentSelectionModeControl({
  snapshot,
  session,
  onModeChange,
  disabled = false,
  className,
}: AgentSelectionModeControlProps) {
  const { t } = useTranslation();
  const idPrefix = useId();
  const showInstallMode = shouldShowInstallMode(snapshot);

  if (!showInstallMode) return null;

  return (
    <div className={cn('flex min-w-0 flex-wrap items-center gap-3', className)}>
      <span className="flex shrink-0 items-center gap-1">
        <span className="text-sm font-medium text-foreground">
          {t('agentSelection.modeTitle')}
        </span>
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              className="inline-flex size-6 shrink-0 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              aria-label={t('agentSelection.modeHelp')}
            >
              <CircleHelp className="size-3.5" aria-hidden="true" />
            </button>
          </TooltipTrigger>
          <TooltipContent className="max-w-72 text-xs leading-5">
            {t('agentSelection.modeHelp')}
          </TooltipContent>
        </Tooltip>
      </span>
      <RadioGroup
        value={session.mode}
        onValueChange={(value) => onModeChange(value as InstallMode)}
        disabled={disabled}
        aria-label={t('agentSelection.modeTitle')}
        className={cn(
          'grid w-60 max-w-full grid-cols-2 gap-1 rounded-md bg-muted p-1',
          disabled && 'opacity-50',
        )}
      >
        <Mode id={`${idPrefix}-symlink`} value="symlink" icon={Link2} label={t('agentSelection.linkRecommended')} />
        <Mode id={`${idPrefix}-copy`} value="copy" icon={Copy} label={t('agentSelection.copy')} />
      </RadioGroup>
    </div>
  );
}

function Mode({
  id,
  value,
  icon: Icon,
  label,
}: {
  id: string;
  value: InstallMode;
  icon: typeof Link2;
  label: string;
}) {
  return (
    <Label
      htmlFor={id}
      className="flex h-7 cursor-pointer items-center justify-center gap-1.5 rounded px-2 text-xs font-medium has-data-[state=checked]:bg-background has-data-[state=checked]:shadow-xs"
    >
      <RadioGroupItem id={id} value={value} className="sr-only" />
      <Icon className="size-3.5" aria-hidden="true" />
      {label}
    </Label>
  );
}
