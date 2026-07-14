import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { FolderOpen, Trash2, Plus } from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { Button } from '@/components/ui/button';
import { EnvironmentSelect } from '@/components/environments/EnvironmentSelect';
import { environmentKey, useEnvironmentStore } from '@/stores/environment';
import { mapEnvironmentPath } from '@/hooks/useTauriApi';
import type { EnvironmentRef, ProjectBinding } from '@/bindings';
import { useMutationStore } from '@/stores/mutation';

interface ProjectRowProps {
  project: ProjectBinding;
  onRemove: (projectId: string) => void;
  writeBlocked: boolean;
}

function ProjectRow({ project, onRemove, writeBlocked }: ProjectRowProps) {
  const { t } = useTranslation();
  const basename = project.displayName
    ?? project.nativePath.split(/[/\\]/).pop()
    ?? project.nativePath;

  return (
    <div className="group flex items-center justify-between px-4 py-3 my-0.5 mx-1.5 rounded-md transition-colors hover:bg-muted/30">
      <div className="flex items-center gap-3 sm:gap-3.5 min-w-0">
        <div className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-md bg-muted/45 text-muted-foreground transition-colors group-hover:text-foreground">
          <FolderOpen className="h-4 w-4" />
        </div>
        <div className="flex flex-col min-w-0">
          <span className="text-sm font-semibold text-foreground truncate">{basename}</span>
          <span className="text-[10px] font-mono text-muted-foreground truncate opacity-80 mt-0.5">
            {project.nativePath}
          </span>
        </div>
      </div>
      <Button
        variant="ghost"
        size="icon"
        className="h-8 w-8 text-muted-foreground/50 hover:text-destructive hover:bg-destructive/10 cursor-pointer flex-shrink-0 opacity-0 group-hover:opacity-100 focus:opacity-100 transition-all"
        onClick={() => onRemove(project.id)}
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
    } catch (error) {
      console.error('Failed to select environment:', error);
    }
  };

  const handleAddProject = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: t('settings.addProject'),
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

  const handleRemoveProject = async (projectId: string) => {
    try {
      await removeProject(projectId, selectedEnvironment);
    } catch (error) {
      console.error('Failed to remove project:', error);
    }
  };

  return (
    <div className="space-y-5">
      <header className="flex items-start justify-between gap-4">
        <div className="space-y-1">
          <h2 className="text-lg font-semibold tracking-tight text-foreground">
            {t('settings.projects')}
          </h2>
          <p className="text-sm text-muted-foreground">
            {t('settings.projectsHint')}
          </p>
        </div>
        <Button
          size="sm"
          className="h-8 cursor-pointer gap-1.5 px-3 text-xs font-medium"
          onClick={handleAddProject}
          aria-label={t('settings.addProject')}
          disabled={writeBlocked || selectedStatus === 'connecting' || selectedStatus === 'unavailable' || selectedStatus === 'error'}
        >
          <Plus className="h-3.5 w-3.5" />
          {t('settings.addProject')}
        </Button>
      </header>

      <EnvironmentSelect
        environments={environments}
        value={selectedEnvironment}
        onChange={handleEnvironmentChange}
        className="h-9 w-full max-w-xs rounded-md border border-border/60 bg-background px-3 text-sm text-foreground"
      />

      <section className="overflow-hidden rounded-lg border border-border/60 bg-background">
        {loadError ? (
          <div className="flex flex-col items-center px-6 py-10 text-center">
            <p className="text-sm text-muted-foreground">{t('context.projectsLoadError')}</p>
            <Button
              variant="link"
              size="sm"
              onClick={() => void refreshProjects(selectedEnvironment).catch(() => undefined)}
            >
              {t('context.environmentRetry')}
            </Button>
          </div>
        ) : !isLoaded ? (
          <div className="px-6 py-10 text-center text-sm text-muted-foreground">
            {t('common.loading')}
          </div>
        ) : projects.length === 0 ? (
          <div className="flex flex-col items-center px-6 py-10 text-center">
            <div className="mb-3 flex h-10 w-10 items-center justify-center rounded-md bg-muted/50 text-muted-foreground">
              <FolderOpen className="h-5 w-5" />
            </div>
            <p className="mb-1 text-sm font-medium text-foreground">
              {t('settings.projectsEmpty')}
            </p>
            <p className="max-w-[260px] text-xs leading-5 text-muted-foreground">
              {t('settings.projectsEmptyHint')}
            </p>
          </div>
        ) : (
          <div className="divide-y divide-border/50">
            {projects.map((project) => (
              <ProjectRow
                key={project.id}
                project={project}
                onRemove={(projectId) => void handleRemoveProject(projectId)}
                writeBlocked={writeBlocked}
              />
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
