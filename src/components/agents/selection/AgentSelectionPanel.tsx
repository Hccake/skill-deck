import { useTranslation } from 'react-i18next';
import type { ReactNode } from 'react';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { cn } from '@/lib/utils';
import { AgentSelectionModeControl } from './AgentSelectionModeControl';
import {
  AgentSelectionUnavailableNotice,
  AgentSelectionView,
} from './AgentSelectionView';
import {
  useAgentSelectionPresentation,
  type AgentSelectionUsage,
} from './useAgentSelectionPresentation';
import type {
  AgentSelectionEnvelope,
  AgentSelectionSessionController,
} from '@/hooks/useAgentSelectionSession';

const ERROR_MESSAGE_KEYS: Record<AgentSelectionUsage, string> = {
  install: 'addSkill.agents.loadError',
  copyToProject: 'skills.copyToProject.agentsLoadError',
  manage: 'skills.manageAgents.previewError',
};

export function AgentSelectionPanel<TSnapshot extends AgentSelectionEnvelope>({
  usage,
  controller,
  disabled = false,
  emptyMessage,
  className,
  modeClassName,
  modeWrapperClassName,
  showUnavailableNotice = true,
  layout = 'stacked',
  title,
  notice,
}: {
  usage: AgentSelectionUsage;
  controller: AgentSelectionSessionController<TSnapshot>;
  disabled?: boolean;
  emptyMessage?: string;
  className?: string;
  modeClassName?: string;
  modeWrapperClassName?: string;
  showUnavailableNotice?: boolean;
  layout?: 'stacked' | 'wizard';
  title?: string;
  notice?: ReactNode;
}) {
  const { t } = useTranslation();
  const presentation = useAgentSelectionPresentation(usage);

  if (controller.status === 'error') {
    return (
      <Alert>
        <AlertDescription className="flex items-center justify-between gap-3">
          <span>{t(ERROR_MESSAGE_KEYS[usage])}</span>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void controller.retry()}
          >
            {t('common.retry')}
          </Button>
        </AlertDescription>
      </Alert>
    );
  }

  if (controller.status !== 'ready') {
    return (
      <div role="status" aria-live="polite" className="space-y-3">
        <span className="sr-only">{t('common.loading')}</span>
        <Skeleton className="h-10 w-72 max-w-full" />
        <Skeleton className="h-20 w-full" />
        <Skeleton className="h-32 w-full" />
      </div>
    );
  }

  const reconfirmation = controller.requiresReconfirmation ? (
    <Alert role="alert">
      <AlertDescription className="flex items-center justify-between gap-3">
        <span>{t('agentSelection.selectionChanged')}</span>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="shrink-0"
          onClick={controller.confirmCurrentSelection}
        >
          {t('agentSelection.confirmCurrentSelection')}
        </Button>
      </AlertDescription>
    </Alert>
  ) : null;
  const mode = (
    <div data-slot="agent-selection-mode-bar" className={cn('empty:hidden', modeWrapperClassName)}>
      <AgentSelectionModeControl
        snapshot={controller.selection}
        session={controller.session}
        onModeChange={controller.setMode}
        disabled={disabled}
        className={modeClassName}
      />
    </div>
  );
  const unavailableNotice = showUnavailableNotice ? (
    <AgentSelectionUnavailableNotice snapshot={controller.selection} />
  ) : null;
  const view = (
    <AgentSelectionView
      presentation={presentation}
      snapshot={controller.selection}
      session={controller.session}
      optionStates={controller.optionStates}
      emptyMessage={emptyMessage}
      disabled={disabled}
      onOptionChange={controller.setOptionSelected}
      onGroupChange={controller.setGroupSelected}
      onOtherExpandedChange={controller.setOtherAgentsExpanded}
      onAdditionalExpandedChange={controller.setAdditionalInstallExpanded}
      onGroupExpandedChange={controller.setGroupExpanded}
    />
  );

  if (layout === 'wizard') {
    return (
      <div className={cn(
        'grid h-full min-h-0 w-full grid-rows-[auto_auto_minmax(0,1fr)] overflow-hidden bg-background',
        className,
      )}>
        <div className="space-y-3 px-8 pt-5 empty:hidden">
          {notice}
          {unavailableNotice}
        </div>
        <header className="flex min-w-0 flex-wrap items-center justify-between gap-x-6 gap-y-3 border-b px-8 py-4">
          {title ? <h2 className="text-base font-semibold">{title}</h2> : null}
          {mode}
        </header>
        <div className="min-h-0 overflow-y-auto overflow-x-hidden overscroll-contain px-8 py-5">
          {reconfirmation ? <div className="mb-4">{reconfirmation}</div> : null}
          {view}
        </div>
      </div>
    );
  }

  return (
    <div className={cn('space-y-6', className)}>
      {notice}
      {reconfirmation}
      {mode}
      {unavailableNotice}
      {view}
    </div>
  );
}
