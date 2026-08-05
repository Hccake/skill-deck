import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { AgentSelectionToolbar } from '@/components/agents/selection/AgentSelectionToolbar';
import { AgentSelectionView } from '@/components/agents/selection/AgentSelectionView';
import {
  createAgentSelectionSession,
  toggleSelectionGroup,
  toggleSelectionItem,
} from '@/lib/agent-selection-session';
import type { InstallTargetOptionsController } from '@/hooks/useInstallTargetOptions';
import type { WizardState } from './types';

interface OptionsStepProps {
  state: WizardState;
  updateState: (updates: Partial<WizardState>) => void;
  targetOptions: InstallTargetOptionsController;
}

export function OptionsStep({ state, updateState, targetOptions }: OptionsStepProps) {
  const { t } = useTranslation();
  const loaded = targetOptions.status === 'ready' ? targetOptions.snapshot : null;
  const session = useMemo(() => {
    if (!loaded) return null;
    return {
      ...createAgentSelectionSession(loaded.selection, state.mode),
      selectedItemIds: state.selectedAgentItemIds,
      otherAgentsExpanded: state.otherAgentsExpanded,
      additionalInstallExpanded: state.additionalAgentsExpanded,
      expandedGroupIds: state.expandedAgentGroupIds,
      requiresReconfirmation: state.selectionRequiresReconfirmation,
    };
  }, [loaded, state.additionalAgentsExpanded, state.expandedAgentGroupIds, state.mode, state.otherAgentsExpanded, state.selectedAgentItemIds, state.selectionRequiresReconfirmation]);

  if (!loaded || !session) {
    return (
      <div className="mx-auto max-w-2xl space-y-3 py-4">
        <h2 className="text-base font-semibold">{t('agentSelection.title')}</h2>
        {targetOptions.status === 'error' ? (
          <Alert>
            <AlertDescription className="flex items-center justify-between gap-3">
              <span>{t('addSkill.agents.loadError')}</span>
              <Button type="button" variant="outline" size="sm" onClick={() => void targetOptions.retry()}>
                {t('common.retry')}
              </Button>
            </AlertDescription>
          </Alert>
        ) : (
          <p role="status" className="text-sm text-muted-foreground">{t('common.loading')}</p>
        )}
      </div>
    );
  }

  const publish = (next: typeof session) => updateState({
    selectedAgentItemIds: next.selectedItemIds,
    otherAgentsExpanded: next.otherAgentsExpanded,
    additionalAgentsExpanded: next.additionalInstallExpanded,
    expandedAgentGroupIds: next.expandedGroupIds,
    selectionRequiresReconfirmation: state.selectionRequiresReconfirmation,
  });

  return (
    <div className="mx-auto flex h-full min-h-0 w-full max-w-3xl flex-col overflow-hidden rounded-md border bg-background">
      {loaded.defaultSelectionWarning ? (
        <Alert className="m-4 mb-0">
          <AlertDescription>{t('addSkill.agents.defaultLoadWarning')}</AlertDescription>
        </Alert>
      ) : null}
      <AgentSelectionToolbar
        snapshot={loaded.selection}
        session={session}
        onModeChange={(mode) => updateState({ mode })}
      />
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-6 py-5">
        {state.selectionRequiresReconfirmation ? (
          <Alert className="mb-4" role="alert">
            <AlertDescription className="flex items-center justify-between gap-3">
              <span>{t('agentSelection.selectionChanged')}</span>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="shrink-0"
                onClick={() => updateState({ selectionRequiresReconfirmation: false })}
              >
                {t('agentSelection.confirmCurrentSelection')}
              </Button>
            </AlertDescription>
          </Alert>
        ) : null}
        <AgentSelectionView
          snapshot={loaded.selection}
          session={session}
          emptyMessage={t('agentSelection.installEmpty')}
          onItemChange={(itemId, selected) => publish(toggleSelectionItem(session, loaded.selection, itemId, selected))}
          onGroupChange={(groupId, selected) => publish(toggleSelectionGroup(session, loaded.selection, groupId, selected))}
          onOtherExpandedChange={(otherAgentsExpanded) => publish({ ...session, otherAgentsExpanded })}
          onAdditionalExpandedChange={(additionalInstallExpanded) => publish({ ...session, additionalInstallExpanded })}
          onGroupExpandedChange={(groupId, expanded) => publish({
            ...session,
            expandedGroupIds: expanded
              ? [...new Set([...session.expandedGroupIds, groupId])]
              : session.expandedGroupIds.filter((id) => id !== groupId),
          })}
        />
      </div>
    </div>
  );
}
