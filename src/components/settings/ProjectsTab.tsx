import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { FolderOpen, Trash2, Plus } from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { RemoveProjectDialog } from '@/components/projects/RemoveProjectDialog';
import { useProjectWorkspace } from '@/hooks/useProjectWorkspace';
import { useWorkspaceContextStore } from '@/stores/workspace-context';
import {
  captureProjectRemoval,
  type ProjectRemovalRequest,
} from '@/stores/project-removal';
import type { ProjectInfo } from '@/bindings';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import { ProjectIdentity } from '@/components/projects/ProjectIdentity';

interface ProjectRowProps {
  project: ProjectInfo;
  onRemove: (project: ProjectInfo) => void;
  writeBlocked: boolean;
}

function ProjectRow({ project, onRemove, writeBlocked }: ProjectRowProps) {
  const { t } = useTranslation();
  return (
    <div className="group flex items-center justify-between px-4 py-3 my-0.5 mx-1.5 rounded-md transition-colors hover:bg-muted/30">
      <div className="flex items-center gap-3 sm:gap-3.5 min-w-0">
        <div className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-md bg-muted/45 text-muted-foreground transition-colors group-hover:text-foreground">
          <FolderOpen className="h-4 w-4" />
        </div>
        <div className="flex flex-col min-w-0">
          <ProjectIdentity
            project={project}
            nameClassName="text-sm font-semibold text-foreground"
            pathClassName="text-[10px] font-mono text-muted-foreground opacity-80 mt-0.5"
          />
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
  const writeBlocked = useBusinessWriteBlocked();
  const selectedContext = useWorkspaceContextStore((state) => state.selectedContext);
  const transitionActive = useWorkspaceContextStore((state) => state.transition.kind !== 'idle');
  const contextRevision = useWorkspaceContextStore((state) => state.contextRevision);
  const [removalRequest, setRemovalRequest] = useState<ProjectRemovalRequest | null>(null);
  const environment = selectedContext.environment;
  const {
    projects,
    hasCompleteSnapshot,
    error: loadError,
    status: selectedStatus,
    refresh,
    add,
  } = useProjectWorkspace(environment);
  const environmentStatusMessage = selectedStatus === 'connecting'
    ? t('context.environmentConnecting')
    : selectedStatus === 'unavailable' || selectedStatus === 'error'
      ? t('context.environmentUnavailable')
      : null;

  const addProject = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: t('settings.addProject'),
      });
      if (!selected || typeof selected !== 'string') return;
      const result = await add(selected);
      if (result.status === 'failed') toast.error(t('settings.addProjectError'));
    } catch (error) {
      console.error('Failed to add project:', error);
      toast.error(t('settings.addProjectError'));
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
          disabled={
            writeBlocked
            || transitionActive
            || selectedStatus !== 'available'
            || !hasCompleteSnapshot
          }
        >
          <Plus className="h-3.5 w-3.5" />
          {t('settings.addProject')}
        </Button>
      </header>

      <section className="overflow-hidden rounded-lg border border-border/60 bg-background">
        {hasCompleteSnapshot && environmentStatusMessage ? (
          <div
            role="status"
            className="border-b border-border/50 px-4 py-2 text-xs text-muted-foreground"
          >
            {environmentStatusMessage}
          </div>
        ) : null}
        {!hasCompleteSnapshot && environmentStatusMessage ? (
          <div role="status" className="px-6 py-10 text-center text-sm text-muted-foreground">
            {environmentStatusMessage}
          </div>
        ) : !hasCompleteSnapshot && loadError ? (
          <div className="flex flex-col items-center px-6 py-10 text-center">
            <p className="text-sm text-muted-foreground">{t('context.projectsLoadError')}</p>
            <Button
              variant="link"
              size="sm"
              onClick={() => void refresh().catch(() => undefined)}
            >
              {t('context.environmentRetry')}
            </Button>
          </div>
        ) : !hasCompleteSnapshot ? (
          <div className="px-6 py-10 text-center text-sm text-muted-foreground">
            {t('common.loading')}
          </div>
        ) : (
          <>
            {loadError ? (
              <div className="flex items-center justify-between gap-3 border-b border-border/50 px-4 py-2 text-xs text-muted-foreground">
                <p>{t('context.projectsLoadError')}</p>
                <Button
                  variant="link"
                  size="sm"
                  className="h-auto p-0"
                  onClick={() => void refresh().catch(() => undefined)}
                >
                  {t('context.environmentRetry')}
                </Button>
              </div>
            ) : null}
            {projects.length === 0 ? (
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
          </>
        )}
      </section>

      <RemoveProjectDialog request={removalRequest} onClose={() => setRemovalRequest(null)} />
    </div>
  );
}
