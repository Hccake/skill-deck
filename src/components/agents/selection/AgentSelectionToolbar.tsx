import { Copy, Link2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { AgentSelectionSnapshot, InstallMode } from '@/bindings';
import { Label } from '@/components/ui/label';
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group';
import { cn } from '@/lib/utils';
import {
  isInstallModeDisabled,
  shouldShowInstallMode,
  type AgentSelectionSession,
} from '@/lib/agent-selection-session';

interface AgentSelectionToolbarProps {
  snapshot: AgentSelectionSnapshot;
  session: AgentSelectionSession;
  onModeChange: (mode: InstallMode) => void;
  disabled?: boolean;
}

export function AgentSelectionToolbar({
  snapshot,
  session,
  onModeChange,
  disabled = false,
}: AgentSelectionToolbarProps) {
  const { t } = useTranslation();
  const showInstallMode = shouldShowInstallMode(snapshot);
  const modeDisabled = disabled || isInstallModeDisabled(session, snapshot);

  return (
    <div className="flex min-w-0 flex-col items-stretch gap-2 border-b px-6 py-3 sm:flex-row sm:items-center sm:justify-between sm:gap-4">
      <Label className="shrink-0 text-sm font-semibold">
        {t('agentSelection.title')}
      </Label>
      {showInstallMode ? (
        <RadioGroup
          value={session.mode}
          onValueChange={(value) => onModeChange(value as InstallMode)}
          disabled={modeDisabled}
          aria-label={t('agentSelection.modeTitle')}
          className={cn(
            'grid w-full grid-cols-2 gap-1 rounded-md bg-muted p-1 sm:w-[15rem]',
            modeDisabled && 'opacity-50',
          )}
        >
          <Mode value="symlink" icon={Link2} label={t('agentSelection.link')} />
          <Mode value="copy" icon={Copy} label={t('agentSelection.copy')} />
        </RadioGroup>
      ) : null}
    </div>
  );
}

function Mode({
  value,
  icon: Icon,
  label,
}: {
  value: InstallMode;
  icon: typeof Link2;
  label: string;
}) {
  const id = `agent-selection-mode-${value}`;
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
