import { useTranslation } from 'react-i18next';

export type AgentSelectionUsage = 'install' | 'manage' | 'copyToProject' | 'libraryApplication';

export interface AgentSelectionPresentation {
  automatic: {
    title: string;
    help: string;
  };
  selectable: {
    title: string;
    help: string;
  };
  ownDirectory: {
    title: string;
    description: string;
    selectedCount: (count: number) => string;
  };
}

export function useAgentSelectionPresentation(
  usage: AgentSelectionUsage,
): AgentSelectionPresentation {
  const { t } = useTranslation();

  return {
    automatic: {
      title: t(`agentSelection.automatic.${usage}.title`),
      help: t(`agentSelection.automatic.${usage}.help`),
    },
    selectable: {
      title: t('agentSelection.selectable.title'),
      help: t('agentSelection.selectable.help'),
    },
    ownDirectory: {
      title: t('agentSelection.ownDirectory.title'),
      description: t(`agentSelection.ownDirectory.${usage}.description`),
      selectedCount: (count) => t('agentSelection.ownDirectory.selectedCount', { count }),
    },
  };
}
