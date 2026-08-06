import { useMemo } from 'react';
import { LoaderCircle, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { environmentKey, useEnvironmentStore } from '@/stores/environment';
import { useProjectStore } from '@/stores/projects';
import { useMutationStore } from '@/stores/mutation';
import {
  selectInstallWizardSessionBlocksWrites,
  useInstallWizardSessionStore,
} from '@/stores/install-wizard-session';
import { useMutationMonitor } from '@/hooks/useMutationMonitor';
import { formatMutationStatus } from '@/lib/mutationStatus';
import { projectDisplayName } from '@/lib/projects/presentation';

interface MutationStatusBarProps {
  pollIntervalMs?: number;
}

export function MutationStatusBar({ pollIntervalMs = 2_000 }: MutationStatusBarProps) {
  const { t } = useTranslation();
  const activeMutation = useMutationStore((state) => state.activeMutation);
  const cancelling = useMutationStore((state) => state.cancelling);
  const cancelActiveMutation = useMutationStore((state) => state.cancelActiveMutation);
  const installWizardBlocksCancellation = useInstallWizardSessionStore(
    selectInstallWizardSessionBlocksWrites,
  );
  const environments = useEnvironmentStore((state) => state.environments);
  const projectsByEnvironment = useProjectStore((state) => state.projectsByEnvironment);

  useMutationMonitor(pollIntervalMs);

  const labels = useMemo(() => {
    if (!activeMutation) return null;

    const key = environmentKey(activeMutation.context.environment);
    const environment = environments.find(
      (entry) => environmentKey(entry.environment) === key,
    );
    const environmentLabel = environment?.displayName
      ?? (activeMutation.context.environment.kind === 'host'
        ? t('mutation.host')
        : activeMutation.context.environment.distro_name);

    if (activeMutation.context.scope.scope === 'global') {
      return { environmentLabel, scopeLabel: t('context.global') };
    }

    const projectId = activeMutation.context.scope.project_id;
    const project = projectsByEnvironment[key]?.find(
      (entry) => entry.binding.id === projectId,
    );
    return {
      environmentLabel,
      scopeLabel: project ? projectDisplayName(project) : projectId,
    };
  }, [activeMutation, environments, projectsByEnvironment, t]);

  if (!activeMutation || !labels) return null;

  return (
    <div
      role="status"
      className="flex h-9 flex-shrink-0 items-center gap-2 border-t bg-muted/40 px-4 text-xs text-muted-foreground"
    >
      <span
        data-testid="mutation-spinner"
        className="inline-flex flex-shrink-0 animate-spin text-primary"
        aria-hidden="true"
      >
        <LoaderCircle className="h-3.5 w-3.5" />
      </span>
      <span className="min-w-0 flex-1 truncate">
        {t('mutation.status', {
          environment: labels.environmentLabel,
          scope: labels.scopeLabel,
          status: formatMutationStatus(activeMutation, t),
        })}
      </span>
      {cancelling ? (
        <span className="flex-shrink-0">{t('mutation.cancelling')}</span>
      ) : activeMutation.cancelable ? (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 flex-shrink-0 gap-1 px-2"
          disabled={installWizardBlocksCancellation}
          onClick={() => void cancelActiveMutation()}
        >
          <X className="h-3.5 w-3.5" aria-hidden="true" />
          {t('mutation.cancel')}
        </Button>
      ) : null}
    </div>
  );
}
