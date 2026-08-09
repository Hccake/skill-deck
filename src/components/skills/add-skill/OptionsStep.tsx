import { useTranslation } from 'react-i18next';
import type { InstallAgentSelectionSnapshot } from '@/bindings';
import { AgentSelectionPanel } from '@/components/agents/selection/AgentSelectionPanel';
import type { AgentSelectionSessionController } from '@/hooks/useAgentSelectionSession';
import { Alert, AlertDescription } from '@/components/ui/alert';

interface OptionsStepProps {
  agentSelection: AgentSelectionSessionController<InstallAgentSelectionSnapshot>;
}

export function OptionsStep({ agentSelection }: OptionsStepProps) {
  const { t } = useTranslation();

  if (agentSelection.status !== 'ready') {
    return (
      <div className="mx-auto max-w-2xl space-y-3 py-4">
        <h2 className="text-base font-semibold">{t('agentSelection.title')}</h2>
        <AgentSelectionPanel
          usage="install"
          controller={agentSelection}
          emptyMessage={t('agentSelection.installEmpty')}
        />
      </div>
    );
  }

  const warning = agentSelection.snapshot.defaultSelectionWarning ? (
    <Alert>
      <AlertDescription>{t('addSkill.agents.defaultLoadWarning')}</AlertDescription>
    </Alert>
  ) : null;

  return (
    <AgentSelectionPanel
      usage="install"
      controller={agentSelection}
      layout="wizard"
      title={t('agentSelection.installTitle')}
      notice={warning}
      emptyMessage={t('agentSelection.installEmpty')}
    />
  );
}
