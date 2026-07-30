import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { FolderOpen, Trash2, Plus } from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { Button } from '@/components/ui/button';
import { RemoveProjectDialog } from '@/components/projects/RemoveProjectDialog';
import { useEnvironmentStore } from '@/stores/environment';
import { useProjectStore } from '@/stores/projects';
import { useWorkspaceContextStore } from '@/stores/workspace-context';
import {
  captureProjectRemoval,
  type ProjectRemovalRequest,
} from '@/stores/project-removal';
import { environmentKey, sameEnvironment } from '@/lib/context';
import type { ProjectInfo } from '@/bindings';
import { useMutationStore } from '@/stores/mutation';

interface ProjectRowProps {
  project: ProjectInfo;
  onRemove: (project: ProjectInfo) => void;
  writeBlocked: boolean;
}

function ProjectRow({ project, onRemove, writeBlocked }: ProjectRowProps) {
  const { t } = useTranslation();
  const basename = project.binding.displayName
    ?? project.binding.nativePath.split(/[/\\]/).pop()
    ?? project.binding.nativePath;

  return (
    <div className="group flex items-center justify-between px-4 py-3 my-0.5 mx-1.5 rounded-md transition-colors hover:bg-muted/30">
      <div className="flex items-center gap-3 sm:gap-3.5 min-w-0">
        <div className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-md bg-muted/45 text-muted-foreground transition-colors group-hover:text-foreground">
          <FolderOpen className="h-4 w-4" />
        </div>
        <div className="flex flex-col min-w-0">
          <span className="text-sm font-semibold text-foreground truncate">{basename}</span>
          <span className="text-[10px] font-mono text-muted-foreground truncate opacity-80 mt-0.5">
            {project.binding.nativePath}
          </span>
        </div>
      </div>
      <Button
        variant="ghost"
        size="icon"
        className="h-8 w-8 text-muted-foreground/50 hover:text-destructive hover:bg-destructive/10 cursor-pointer flex-shrink-0 opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 focus:opacity-100 transition-colors transition-opacity"
        onClick={() => onRemove(project)}
        aria-label={t('settings.removeProject')}
        disabled={writeBlocked}
      >
        <Trash2 className="h-4 w-4" />
      </Button>
    </div>
  );
}

export function ProjectsTab() {
  const { t } = useTranslation();
  const writeBlocked = useMutationStore((state) => state.activeMutation !== null);
  const environments = useEnvironmentStore((state) => state.environments);
  const selectedContext = useWorkspaceContextStore((state) => state.selectedContext);
  const pendingEnvironment = useWorkspaceContextStore((state) => state.pendingEnvironment);
  const contextRevision = useWorkspaceContextStore((state) => state.contextRevision);
  const projectsByEnvironment = useProjectStore((state) => state.projectsByEnvironment);
  const loadStateByEnvironment = useProjectStore((state) => state.loadStateByEnvironment);
  const errorsByEnvironment = useProjectStore((state) => state.errorsByEnvironment);
  const refresh = useProjectStore((state) => state.refresh);
  const add = useProjectStore((state) => state.add);
  const [removalRequest, setRemovalRequest] = useState<ProjectRemovalRequest | null>(null);
  const environment = selectedContext.environment;
  const key = environmentKey(environment);
  const projects = projectsByEnvironment[key] ?? [];
  const loadState = loadStateByEnvironment[key] ?? 'idle';
  const loadError = errorsByEnvironment[key];
  const selectedStatus = environments.find(
    (entry) => sameEnvironment(entry.environment, environment),
  )?.status;

  useEffect(() => {
    if (loadState === 'idle' && !pendingEnvironment && selectedStatus === 'available') {
      void refresh(environment).catch(() => undefined);
    }
  }, [environment, loadState, pendingEnvironment, refresh, selectedStatus]);

  const addProject = async () => {
    const targetEnvironment = environment;
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: t('settings.addProject'),
      });
      if (!selected || typeof selected !== 'string') return;
      await add(targetEnvironment, selected);
    } catch (error) {
      console.error('Failed to add project:', error);
    }
  };

  return (
    <div className="space-y-5">
      <header className="flex items-start justify-between gap-4">
        <div className="space-y-1">
          <h2 className="text-lg font-semibold tracking-tight text-foreground">
            {t('settings.projects')}
          </h2>
          <p className="text-sm text-muted-foreground">{t('settings.projectsHint')}</p>
        </div>
        <Button
          size="sm"
          className="h-8 cursor-pointer gap-1.5 px-3 text-xs font-medium"
          onClick={() => void addProject()}
          aria-label={t('settings.addProject')}
          disabled={writeBlocked || pendingEnvironment !== null || selectedStatus !== 'available'}
        >
          <Plus className="h-3.5 w-3.5" />
          {t('settings.addProject')}
        </Button>
      </header>

      <section className="overflow-hidden rounded-lg border border-border/60 bg-background">
        {loadError ? (
          <div className="flex flex-col items-center px-6 py-10 text-center">
            <p className="text-sm text-muted-foreground">{t('context.projectsLoadError')}</p>
            <Button
              variant="link"
              size="sm"
              onClick={() => void refresh(environment).catch(() => undefined)}
            >
              {t('context.environmentRetry')}
            </Button>
          </div>
        ) : loadState !== 'ready' ? (
          <div className="px-6 py-10 text-center text-sm text-muted-foreground">
            {t('common.loading')}
          </div>
        ) : projects.length === 0 ? (
          <div className="flex flex-col items-center px-6 py-10 text-center">
            <div className="mb-3 flex h-10 w-10 items-center justify-center rounded-md bg-muted/50 text-muted-foreground">
              <FolderOpen className="h-5 w-5" />
            </div>
            <p className="mb-1 text-sm font-medium text-foreground">{t('settings.projectsEmpty')}</p>
            <p className="max-w-[260px] text-xs leading-5 text-muted-foreground">
              {t('settings.projectsEmptyHint')}
            </p>
          </div>
        ) : (
          <div className="divide-y divide-border/50">
            {projects.map((project) => (
              <ProjectRow
                key={project.binding.id}
                project={project}
                onRemove={(target) => setRemovalRequest(captureProjectRemoval(
                  environment,
                  target,
                  contextRevision,
                ))}
                writeBlocked={writeBlocked}
              />
            ))}
          </div>
        )}
      </section>

      <RemoveProjectDialog request={removalRequest} onClose={() => setRemovalRequest(null)} />
    </div>
  );
}
