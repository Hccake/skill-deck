import { useState } from 'react';
import { TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { useContextStore } from '@/stores/context';
import { environmentKey, useEnvironmentStore } from '@/stores/environment';
import { useMutationStore } from '@/stores/mutation';
import { isCrossStorageProject } from '@/lib/projectStorage';
import type { ProjectBinding } from '@/bindings';

const EMPTY_PROJECTS: ProjectBinding[] = [];

export function CrossStorageWarningBanner() {
  const { t } = useTranslation();
  const selectedContextRef = useContextStore((state) => state.selectedContextRef);
  const environment = selectedContextRef.environment;
  const environmentProjects = useEnvironmentStore((state) => (
    state.projectsByEnvironment[environmentKey(environment)] ?? EMPTY_PROJECTS
  ));
  const suppressWarning = useEnvironmentStore((state) => state.suppressCrossStorageWarning);
  const writeBlocked = useMutationStore((state) => state.activeMutation !== null);
  const [dismissing, setDismissing] = useState(false);

  if (selectedContextRef.scope.scope !== 'project') return null;
  const projectId = selectedContextRef.scope.project_id;

  const project = environmentProjects.find(
    (entry) => entry.id === projectId,
  );
  if (!project
    || project.suppressCrossStorageWarning
    || !isCrossStorageProject(environment, project.nativePath)) {
    return null;
  }

  const handleDismiss = async () => {
    setDismissing(true);
    try {
      await suppressWarning(project.id, environment);
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
          {t('crossStorage.description')}
        </p>
      </div>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="h-7 flex-shrink-0 px-2 text-xs hover:bg-amber-500/10"
        onClick={() => void handleDismiss()}
        disabled={writeBlocked || dismissing}
      >
        {t('crossStorage.dismiss')}
      </Button>
    </div>
  );
}
