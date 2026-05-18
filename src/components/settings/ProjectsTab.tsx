import { useTranslation } from 'react-i18next';
import { FolderOpen, Trash2, Plus } from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { Button } from '@/components/ui/button';
import { useContextStore } from '@/stores/context';

interface ProjectRowProps {
  path: string;
  onRemove?: (path: string) => void;
}

function ProjectRow({ path, onRemove }: ProjectRowProps) {
  const basename = path.split(/[/\\]/).pop() || path;

  return (
    <div className="group flex items-center justify-between px-4 py-3 my-0.5 mx-1.5 rounded-md transition-colors hover:bg-muted/30">
      <div className="flex items-center gap-3 sm:gap-3.5 min-w-0">
        <div className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-md bg-muted/45 text-muted-foreground transition-colors group-hover:text-foreground">
          <FolderOpen className="h-4 w-4" />
        </div>
        <div className="flex flex-col min-w-0">
          <span className="text-sm font-semibold text-foreground truncate">{basename}</span>
          <span className="text-[10px] font-mono text-muted-foreground truncate opacity-80 mt-0.5">{path}</span>
        </div>
      </div>
      <Button
        variant="ghost"
        size="icon"
        className="h-8 w-8 text-muted-foreground/50 hover:text-destructive hover:bg-destructive/10 cursor-pointer flex-shrink-0 opacity-0 group-hover:opacity-100 focus:opacity-100 transition-all"
        onClick={() => onRemove?.(path)}
      >
        <Trash2 className="h-4 w-4" />
      </Button>
    </div>
  );
}

export function ProjectsTab() {
  const { t } = useTranslation();
  const { projects, addProject, removeProject } = useContextStore();

  const handleAddProject = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: t('settings.addProject'),
      });
      if (selected && typeof selected === 'string') {
        await addProject(selected);
      }
    } catch (error) {
      console.error('Failed to open folder picker:', error);
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
        >
          <Plus className="h-3.5 w-3.5" />
          {t('settings.addProject')}
        </Button>
      </header>

      <section className="overflow-hidden rounded-lg border border-border/60 bg-background">
        {projects.length === 0 ? (
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
            {projects.map((path) => (
              <ProjectRow
                key={path}
                path={path}
                onRemove={(path) => removeProject(path)}
              />
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
