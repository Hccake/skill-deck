import { useState } from 'react';
import { TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { useWorkspaceContextStore } from '@/stores/workspace-context';
import { useEnvironmentStore } from '@/stores/environment';
import { useProjectStore } from '@/stores/projects';
import { environmentKey, sameEnvironment } from '@/lib/context';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import type { ProjectInfo } from '@/bindings';

const EMPTY_PROJECTS: ProjectInfo[] = [];

export function CrossStorageWarningBanner() {
  const { t } = useTranslation();
  const selectedContext = useWorkspaceContextStore((state) => state.selectedContext);
  const transitionActive = useWorkspaceContextStore((state) => state.transition.kind !== 'idle');
  const switchEnvironment = useWorkspaceContextStore((state) => state.switchEnvironment);
  const environments = useEnvironmentStore((state) => state.environments);
  const environment = selectedContext.environment;
  const environmentProjects = useProjectStore((state) => (
    state.projectsByEnvironment[environmentKey(environment)] ?? EMPTY_PROJECTS
  ));
  const setCrossStorageWarning = useProjectStore((state) => state.setCrossStorageWarning);
  const writeBlocked = useBusinessWriteBlocked();
  const [dismissing, setDismissing] = useState(false);

  if (selectedContext.scope.scope !== 'project') return null;
  const projectId = selectedContext.scope.project_id;

  const project = environmentProjects.find(
    (entry) => entry.binding.id === projectId,
  );
  if (!project
    || project.binding.suppressCrossStorageWarning
    || project.storage.access !== 'crossStorage') {
    return null;
  }
  const owner = project.storage.owner;
  const currentInfo = environments.find(
    (entry) => sameEnvironment(entry.environment, environment),
  );
  const ownerInfo = owner ? environments.find(
    (entry) => sameEnvironment(entry.environment, owner),
  ) : null;
  const currentLabel = currentInfo?.displayName
    ?? (environment.kind === 'host' ? t('crossStorage.hostEnvironment') : environment.distro_name);
  const ownerLabel = owner
    ? ownerInfo?.displayName
      ?? (owner.kind === 'host' ? t('crossStorage.hostEnvironment') : owner.distro_name)
    : t('common.unknown');
  const canSwitchToOwner = owner !== null
    && ownerInfo !== undefined
    && ownerInfo !== null
    && !sameEnvironment(owner, environment);

  const handleDismiss = async () => {
    setDismissing(true);
    try {
      await setCrossStorageWarning(environment, project.binding.id, true);
    } catch (error) {
      console.error('Failed to suppress cross-storage warning:', error);
      toast.error(t('crossStorage.dismissFailed'));
    } finally {
      setDismissing(false);
    }
  };

  return (
    <div
      role="note"
      className="mx-3 mt-3 flex flex-shrink-0 items-start gap-3 rounded-lg border border-amber-500/30 bg-amber-500/8 px-3.5 py-3 text-amber-950 dark:text-amber-100 sm:mx-4"
    >
      <TriangleAlert className="mt-0.5 h-4 w-4 flex-shrink-0 text-amber-600 dark:text-amber-400" aria-hidden="true" />
      <div className="min-w-0 flex-1">
        <p className="text-sm font-semibold">{t('crossStorage.title')}</p>
        <p className="mt-0.5 text-xs leading-5 text-amber-900/75 dark:text-amber-100/70">
          {t('crossStorage.description', {
            environment: currentLabel,
            owner: ownerLabel,
          })}
        </p>
      </div>
      <div className="flex flex-shrink-0 flex-wrap justify-end gap-1.5">
        {canSwitchToOwner ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 px-2 text-xs"
            onClick={() => void switchEnvironment(owner).catch((error) => {
              console.error('Failed to switch to storage owner:', error);
            })}
            disabled={transitionActive}
          >
            {t('crossStorage.switchToOwner', { owner: ownerLabel })}
          </Button>
        ) : null}
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 px-2 text-xs hover:bg-amber-500/10"
          onClick={() => void handleDismiss()}
          disabled={writeBlocked || dismissing}
        >
          {t('crossStorage.dismiss')}
        </Button>
      </div>
    </div>
  );
}
