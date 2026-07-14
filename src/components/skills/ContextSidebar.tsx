import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus, Globe, Folder, FolderOpen, Trash2 } from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { Button } from '@/components/ui/button';
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '@/components/ui/context-menu';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { EnvironmentSelect } from '@/components/environments/EnvironmentSelect';
import { useContextStore } from '@/stores/context';
import { environmentKey, useEnvironmentStore } from '@/stores/environment';
import { mapEnvironmentPath, openInExplorer } from '@/hooks/useTauriApi';
import type { EnvironmentRef, ProjectBinding } from '@/bindings';
import { cn } from '@/lib/utils';
import { useMutationStore } from '@/stores/mutation';

function getProjectName(project: ProjectBinding): string {
  if (project.displayName) return project.displayName;
  const parts = project.nativePath.replace(/\\/g, '/').split('/');
  return parts[parts.length - 1] || project.nativePath;
}

function explorerPath(environment: EnvironmentRef, nativePath: string): string {
  if (environment.kind === 'host') return nativePath;
  const relativePath = nativePath.replace(/^\/+/, '').replace(/\//g, '\\');
  return `\\\\wsl.localhost\\${environment.distro_name}\\${relativePath}`;
}

function GlobalContextItem({ environment }: { environment: EnvironmentRef }) {
  const { t } = useTranslation();
  const selectedContextRef = useContextStore((state) => state.selectedContextRef);
  const selectContextRef = useContextStore((state) => state.selectContextRef);
  const isSelected = environmentKey(selectedContextRef.environment) === environmentKey(environment)
    && selectedContextRef.scope.scope === 'global';

  return (
    <button
      onClick={() => selectContextRef({ environment, scope: { scope: 'global' } }, 'global')}
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
  project: ProjectBinding;
  onRemove: (project: ProjectBinding) => Promise<void>;
  writeBlocked: boolean;
}

function ProjectContextItem({ environment, project, onRemove, writeBlocked }: ProjectContextItemProps) {
  const { t } = useTranslation();
  const selectedContextRef = useContextStore((state) => state.selectedContextRef);
  const selectContextRef = useContextStore((state) => state.selectContextRef);
  const isSelected = environmentKey(selectedContextRef.environment) === environmentKey(environment)
    && selectedContextRef.scope.scope === 'project'
    && selectedContextRef.scope.project_id === project.id;
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const projectName = getProjectName(project);

  const selectProject = () => selectContextRef({
    environment,
    scope: { scope: 'project', project_id: project.id },
  }, project.nativePath);

  const handleOpenInExplorer = async (event?: React.MouseEvent) => {
    event?.stopPropagation();
    try {
      await openInExplorer(explorerPath(environment, project.nativePath));
    } catch (error) {
      console.error('Failed to open in explorer:', error);
    }
  };

  const handleRemove = async () => {
    try {
      await onRemove(project);
      setDeleteDialogOpen(false);
    } catch (error) {
      console.error('Failed to remove project:', error);
    }
  };

  const itemButton = (
    <div
      role="button"
      tabIndex={0}
      onClick={selectProject}
      onKeyDown={(event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          selectProject();
        }
      }}
      className={cn(
        'w-full px-4 py-2 text-left transition-colors group relative cursor-pointer',
        isSelected
          ? 'bg-primary/10 text-primary shadow-[inset_2px_0_0_0_theme(colors.primary.DEFAULT)]'
          : 'text-muted-foreground hover:bg-foreground/[0.04] hover:text-foreground',
      )}
    >
      <div className="flex items-center gap-3">
        <Folder className={cn('h-4 w-4 flex-shrink-0', isSelected ? 'text-primary' : 'text-muted-foreground')} />
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className={cn('text-sm truncate', isSelected ? 'font-bold' : 'font-medium')}>
              {projectName}
            </span>
            <div className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity ml-auto">
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 hover:bg-background/80 cursor-pointer"
                onClick={handleOpenInExplorer}
                aria-label={t('context.openInExplorer')}
              >
                <FolderOpen className="h-3.5 w-3.5" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 hover:bg-destructive/10 hover:text-destructive cursor-pointer"
                onClick={(event) => {
                  event.stopPropagation();
                  setDeleteDialogOpen(true);
                }}
                aria-label={t('context.remove')}
                disabled={writeBlocked}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>
          <span className={cn('text-[10px] truncate block', isSelected ? 'opacity-70' : 'opacity-60')}>
            {project.nativePath}
          </span>
        </div>
      </div>
    </div>
  );

  return (
    <>
      <ContextMenu>
        <ContextMenuTrigger asChild>{itemButton}</ContextMenuTrigger>
        <ContextMenuContent className="w-48">
          <ContextMenuItem onClick={() => handleOpenInExplorer()} className="cursor-pointer">
            <FolderOpen className="h-4 w-4 mr-2" />
            {t('context.openInExplorer')}
          </ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem
            onClick={() => setDeleteDialogOpen(true)}
            className="text-destructive focus:text-destructive cursor-pointer"
            disabled={writeBlocked}
          >
            <Trash2 className="h-4 w-4 mr-2" />
            {t('context.remove')}
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>

      <AlertDialog open={deleteDialogOpen} onOpenChange={setDeleteDialogOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('context.removeConfirm.title')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('context.removeConfirm.description', { name: projectName })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel className="cursor-pointer">
              {t('context.removeConfirm.cancel')}
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={(event) => {
                event.preventDefault();
                void handleRemove();
              }}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90 cursor-pointer"
              disabled={writeBlocked}
            >
              {t('context.removeConfirm.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}

export function ContextSidebar() {
  const { t } = useTranslation();
  const writeBlocked = useMutationStore((state) => state.activeMutation !== null);
  const {
    environments,
    selectedEnvironment,
    projectsByEnvironment,
    projectsLoaded,
    errors,
    selectEnvironment,
    refreshProjects,
    addProject,
    removeProject,
  } = useEnvironmentStore();
  const selectedContextRef = useContextStore((state) => state.selectedContextRef);
  const selectContextRef = useContextStore((state) => state.selectContextRef);
  const selectedKey = environmentKey(selectedEnvironment);
  const projects = projectsByEnvironment[selectedKey] ?? [];
  const isLoaded = projectsLoaded[selectedKey] ?? false;
  const loadError = errors[selectedKey];
  const selectedStatus = environments.find(
    (entry) => environmentKey(entry.environment) === selectedKey,
  )?.status;

  useEffect(() => {
    if (!isLoaded && !loadError && selectedStatus !== 'connecting') {
      void refreshProjects(selectedEnvironment).catch(() => undefined);
    }
  }, [isLoaded, loadError, refreshProjects, selectedEnvironment, selectedKey, selectedStatus]);

  const handleEnvironmentChange = async (environment: EnvironmentRef) => {
    try {
      await selectEnvironment(environment);
      selectContextRef({ environment, scope: { scope: 'global' } }, 'global');
    } catch (error) {
      console.error('Failed to select environment:', error);
    }
  };

  const handleAddProject = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: t('context.addProject'),
      });
      if (!selected || typeof selected !== 'string') return;
      const nativePath = selectedEnvironment.kind === 'wsl'
        ? await mapEnvironmentPath(selectedEnvironment, selected)
        : selected;
      await addProject(nativePath, selectedEnvironment);
    } catch (error) {
      console.error('Failed to add project:', error);
    }
  };

  const handleRemoveProject = async (project: ProjectBinding) => {
    await removeProject(project.id, selectedEnvironment);
    const removingSelectedProject = environmentKey(selectedContextRef.environment) === selectedKey
      && selectedContextRef.scope.scope === 'project'
      && selectedContextRef.scope.project_id === project.id;
    if (removingSelectedProject) {
      selectContextRef({
        environment: selectedEnvironment,
        scope: { scope: 'global' },
      }, 'global');
    }
  };

  return (
    <aside className="skills-context-sidebar flex flex-col h-full bg-canvas flex-shrink-0 border-r border-border/50">
      <div data-testid="context-sidebar-scroll" className="flex-1 overflow-y-auto space-y-4 pt-5">
        <div className="px-4">
          <EnvironmentSelect
            environments={environments}
            value={selectedEnvironment}
            onChange={handleEnvironmentChange}
          />
        </div>

        <div>
          <h3 className="px-4 mb-2 text-xs font-semibold text-muted-foreground/80">
            {t('context.sectionGlobal')}
          </h3>
          <GlobalContextItem environment={selectedEnvironment} />
        </div>

        <div>
          <h3 className="px-4 mb-2 text-xs font-semibold text-muted-foreground/80">
            {t('context.sectionProjects')}
          </h3>
          {loadError ? (
            <div className="px-4 py-2 text-xs text-muted-foreground">
              <p>{t('context.projectsLoadError')}</p>
              <button
                className="mt-2 text-primary hover:underline cursor-pointer"
                onClick={() => void refreshProjects(selectedEnvironment).catch(() => undefined)}
              >
                {t('context.environmentRetry')}
              </button>
            </div>
          ) : !isLoaded ? (
            <p className="px-4 py-2 text-xs text-muted-foreground">
              {t('common.loading')}
            </p>
          ) : projects.length === 0 ? (
            <p className="px-4 py-2 text-xs text-muted-foreground">
              {t('context.noProjects')}
            </p>
          ) : (
            <div className="space-y-0.5">
              {projects.map((project) => (
                <ProjectContextItem
                  key={project.id}
                  environment={selectedEnvironment}
                  project={project}
                  onRemove={handleRemoveProject}
                  writeBlocked={writeBlocked}
                />
              ))}
            </div>
          )}
        </div>
      </div>

      <div className="p-4">
        <button
          className="w-full flex items-center justify-start gap-1.5 px-3 py-2 rounded-md hover:bg-foreground/[0.04] transition-colors text-muted-foreground hover:text-foreground font-semibold text-sm cursor-pointer"
          onClick={handleAddProject}
          aria-label={t('context.addProject')}
          disabled={writeBlocked || selectedStatus === 'connecting' || selectedStatus === 'unavailable' || selectedStatus === 'error'}
        >
          <Plus className="h-4 w-4" />
          {t('context.addProject')}
        </button>
      </div>
    </aside>
  );
}
