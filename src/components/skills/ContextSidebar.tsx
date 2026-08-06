import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus, Globe, Folder, FolderOpen, Trash2 } from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '@/components/ui/context-menu';
import { RemoveProjectDialog } from '@/components/projects/RemoveProjectDialog';
import { useWorkspaceContextStore } from '@/stores/workspace-context';
import { useProjectWorkspace } from '@/hooks/useProjectWorkspace';
import {
  captureProjectRemoval,
  type ProjectRemovalRequest,
} from '@/stores/project-removal';
import { sameEnvironment } from '@/lib/context';
import { openConfigResource } from '@/hooks/useTauriApi';
import type { EnvironmentRef, ProjectInfo } from '@/bindings';
import { cn } from '@/lib/utils';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import { ProjectIdentity } from '@/components/projects/ProjectIdentity';

interface GlobalContextItemProps {
  buttonRef: React.Ref<HTMLButtonElement>;
}

function GlobalContextItem({ buttonRef }: GlobalContextItemProps) {
  const { t } = useTranslation();
  const selectedContext = useWorkspaceContextStore((state) => state.selectedContext);
  const selectGlobal = useWorkspaceContextStore((state) => state.selectGlobal);
  const isSelected = selectedContext.scope.scope === 'global';

  return (
    <button
      ref={buttonRef}
      type="button"
      onClick={selectGlobal}
      className={cn(
        'w-full px-4 py-2 text-left transition-colors cursor-pointer',
        isSelected
          ? 'bg-primary/10 text-primary'
          : 'text-muted-foreground hover:bg-foreground/[0.02] hover:text-foreground',
      )}
    >
      <div className="flex items-center gap-3">
        <Globe className="h-4 w-4 flex-shrink-0" />
        <div className="min-w-0">
          <span className={cn('text-sm', isSelected ? 'font-bold' : 'font-medium')}>
            {t('context.global')}
          </span>
          <p className="text-[10px] text-muted-foreground/60 truncate mt-0.5">
            {t('context.globalSubtitle')}
          </p>
        </div>
      </div>
    </button>
  );
}

interface ProjectContextItemProps {
  environment: EnvironmentRef;
  project: ProjectInfo;
  onRequestRemove: (project: ProjectInfo) => void;
  writeBlocked: boolean;
  selectionRef: (element: HTMLButtonElement | null) => void;
}

function ProjectContextItem({
  environment,
  project,
  onRequestRemove,
  writeBlocked,
  selectionRef,
}: ProjectContextItemProps) {
  const { t } = useTranslation();
  const selectedContext = useWorkspaceContextStore((state) => state.selectedContext);
  const selectProject = useWorkspaceContextStore((state) => state.selectProject);
  const isSelected = sameEnvironment(selectedContext.environment, environment)
    && selectedContext.scope.scope === 'project'
    && selectedContext.scope.project_id === project.binding.id;

  const openProject = async (event?: React.MouseEvent) => {
    event?.stopPropagation();
    try {
      await openConfigResource({
        environment,
        scope: { scope: 'project', project_id: project.binding.id },
      }, 'contextRoot');
    } catch (error) {
      console.error('Failed to open in explorer:', error);
    }
  };

  const projectRow = (
    <div
      data-project-id={project.binding.id}
      className={cn(
        'project-context-item group relative flex w-full transition-colors',
        isSelected
          ? 'bg-primary/10 text-primary shadow-[inset_2px_0_0_0_theme(colors.primary.DEFAULT)]'
          : 'text-muted-foreground hover:bg-foreground/[0.04] hover:text-foreground',
      )}
    >
      <button
        ref={selectionRef}
        type="button"
        onClick={() => selectProject(project.binding.id)}
        className="flex min-w-0 flex-1 items-center gap-3 px-4 py-2 pr-16 text-left cursor-pointer"
      >
        <Folder className={cn('h-4 w-4 flex-shrink-0', isSelected ? 'text-primary' : 'text-muted-foreground')} />
        <span className="min-w-0 flex-1">
          <ProjectIdentity
            project={project}
            nameClassName={cn('text-sm', isSelected ? 'font-bold' : 'font-medium')}
            pathClassName={cn('text-[10px]', isSelected ? 'opacity-70' : 'opacity-60')}
          />
        </span>
      </button>
      <div className="absolute right-3 top-1/2 flex -translate-y-1/2 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100">
        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6 hover:bg-background/80 cursor-pointer"
          onClick={openProject}
          aria-label={t('context.openInExplorer')}
          title={t('context.openInExplorer')}
        >
          <FolderOpen className="h-3.5 w-3.5" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6 hover:bg-destructive/10 hover:text-destructive cursor-pointer"
          onClick={() => onRequestRemove(project)}
          aria-label={t('context.remove')}
          title={t('context.remove')}
          disabled={writeBlocked}
        >
          <Trash2 className="h-3.5 w-3.5" />
        </Button>
      </div>
    </div>
  );

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{projectRow}</ContextMenuTrigger>
      <ContextMenuContent className="w-48">
        <ContextMenuItem onClick={() => void openProject()} className="cursor-pointer">
          <FolderOpen className="h-4 w-4 mr-2" />
          {t('context.openInExplorer')}
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem
          onClick={() => onRequestRemove(project)}
          className="text-destructive focus:text-destructive cursor-pointer"
          disabled={writeBlocked}
        >
          <Trash2 className="h-4 w-4 mr-2" />
          {t('context.remove')}
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

export function ContextSidebar() {
  const { t } = useTranslation();
  const writeBlocked = useBusinessWriteBlocked();
  const selectedContext = useWorkspaceContextStore((state) => state.selectedContext);
  const transitionActive = useWorkspaceContextStore((state) => state.transition.kind !== 'idle');
  const contextRevision = useWorkspaceContextStore((state) => state.contextRevision);
  const [removalRequest, setRemovalRequest] = useState<ProjectRemovalRequest | null>(null);
  const globalButtonRef = useRef<HTMLButtonElement>(null);
  const projectButtonRefs = useRef(new Map<string, HTMLButtonElement>());
  const focusAfterRemovalRef = useRef<string | null>(null);
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
        title: t('context.addProject'),
      });
      if (!selected || typeof selected !== 'string') return;
      const result = await add(selected);
      if (result.status === 'failed') toast.error(t('context.addProjectError'));
    } catch (error) {
      console.error('Failed to add project:', error);
      toast.error(t('context.addProjectError'));
    }
  };

  const requestRemoval = (target: ProjectInfo) => {
    const index = projects.findIndex((entry) => entry.binding.id === target.binding.id);
    focusAfterRemovalRef.current = projects[index + 1]?.binding.id
      ?? projects[index - 1]?.binding.id
      ?? null;
    setRemovalRequest(captureProjectRemoval(environment, target, contextRevision));
  };

  const restoreFocusAfterRemoval = () => {
    const targetId = focusAfterRemovalRef.current;
    focusAfterRemovalRef.current = null;
    queueMicrotask(() => {
      const target = targetId ? projectButtonRefs.current.get(targetId) : null;
      (target ?? globalButtonRef.current)?.focus();
    });
  };

  return (
    <aside className="skills-context-sidebar flex flex-col h-full bg-canvas flex-shrink-0 border-r border-border/50">
      <div className="flex-shrink-0 pt-5">
        <div>
          <h3 className="px-4 mb-2 text-xs font-semibold text-muted-foreground/80">
            {t('context.sectionGlobal')}
          </h3>
          <GlobalContextItem buttonRef={globalButtonRef} />
        </div>
      </div>

      <div className="flex min-h-0 flex-1 flex-col pt-4">
        <h3 className="flex-shrink-0 px-4 mb-2 text-xs font-semibold text-muted-foreground/80">
          {t('context.sectionProjects')}
        </h3>
        <div data-testid="context-sidebar-scroll" className="min-h-0 flex-1 overflow-y-auto">
          {hasCompleteSnapshot && environmentStatusMessage ? (
            <p role="status" className="px-4 py-2 text-xs text-muted-foreground">
              {environmentStatusMessage}
            </p>
          ) : null}
          {!hasCompleteSnapshot && environmentStatusMessage ? (
            <p role="status" className="px-4 py-2 text-xs text-muted-foreground">
              {environmentStatusMessage}
            </p>
          ) : !hasCompleteSnapshot && loadError ? (
            <div className="px-4 py-2 text-xs text-muted-foreground">
              <p>{t('context.projectsLoadError')}</p>
              <button
                type="button"
                className="mt-2 text-primary hover:underline cursor-pointer"
                onClick={() => void refresh().catch(() => undefined)}
              >
                {t('context.environmentRetry')}
              </button>
            </div>
          ) : !hasCompleteSnapshot ? (
            <p className="px-4 py-2 text-xs text-muted-foreground">{t('common.loading')}</p>
          ) : (
            <>
              {loadError ? (
                <div className="flex items-center justify-between gap-2 px-4 py-2 text-xs text-muted-foreground">
                  <p>{t('context.projectsLoadError')}</p>
                  <button
                    type="button"
                    className="flex-shrink-0 text-primary hover:underline cursor-pointer"
                    onClick={() => void refresh().catch(() => undefined)}
                  >
                    {t('context.environmentRetry')}
                  </button>
                </div>
              ) : null}
              {projects.length === 0 ? (
                <p className="px-4 py-2 text-xs text-muted-foreground">{t('context.noProjects')}</p>
              ) : (
                <div className="space-y-0.5">
                  {projects.map((project) => (
                    <ProjectContextItem
                      key={project.binding.id}
                      environment={environment}
                      project={project}
                      onRequestRemove={requestRemoval}
                      writeBlocked={writeBlocked}
                      selectionRef={(element) => {
                        if (element) projectButtonRefs.current.set(project.binding.id, element);
                        else projectButtonRefs.current.delete(project.binding.id);
                      }}
                    />
                  ))}
                </div>
              )}
            </>
          )}
        </div>
      </div>

      <div className="p-4">
        <button
          type="button"
          className="w-full flex items-center justify-start gap-1.5 px-3 py-2 rounded-md hover:bg-foreground/[0.04] transition-colors text-muted-foreground hover:text-foreground font-semibold text-sm cursor-pointer"
          onClick={() => void addProject()}
          aria-label={t('context.addProject')}
          disabled={
            writeBlocked
            || transitionActive
            || selectedStatus !== 'available'
            || !hasCompleteSnapshot
          }
        >
          <Plus className="h-4 w-4" />
          {t('context.addProject')}
        </button>
      </div>

      <RemoveProjectDialog
        request={removalRequest}
        onClose={() => setRemovalRequest(null)}
        onRemoved={restoreFocusAfterRemoval}
      />
    </aside>
  );
}
