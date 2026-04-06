import { useTranslation } from 'react-i18next';
import { FolderOpen, Trash2, Plus, Briefcase } from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { Card, CardContent } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { useContextStore } from '@/stores/context';

interface ProjectRowProps {
  path: string;
  onRemove?: (path: string) => void;
}

function ProjectRow({ path, onRemove }: ProjectRowProps) {
  const basename = path.split(/[/\\]/).pop() || path;

  return (
    <div className="flex items-center justify-between py-2.5 px-3 sm:px-4 group hover:bg-muted/30 transition-colors">
      <div className="flex items-center gap-3 sm:gap-3.5 min-w-0">
        <div className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-[10px] bg-muted/60 text-muted-foreground group-hover:bg-background group-hover:text-foreground transition-colors border border-border/40 shadow-sm">
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
    <div className="space-y-5 sm:space-y-6">
      <section>
        <div className="flex items-center justify-between mb-3">
          <div className="flex items-center gap-2 sm:gap-2.5">
            <div className="flex h-7 w-7 items-center justify-center rounded-lg bg-accent">
              <Briefcase className="h-4 w-4 text-accent-foreground" />
            </div>
            <div>
              <h2 className="text-sm font-heading font-bold text-foreground">
                {t('settings.projects')}
              </h2>
              <p className="text-xs text-muted-foreground">
                {t('settings.projectsHint')}
              </p>
            </div>
          </div>
          <Button
            size="sm"
            className="gap-1.5 cursor-pointer shadow-sm font-medium h-8 bg-primary/10 text-primary hover:bg-primary/20 transition-all"
            onClick={handleAddProject}
          >
            <Plus className="h-3.5 w-3.5" />
            {t('settings.addProject')}
          </Button>
        </div>

        <Card className="py-0 gap-0 overflow-hidden shadow-sm border-border/60">
          {projects.length === 0 ? (
            <div className="relative bg-muted/10 p-8 sm:p-10 flex flex-col items-center text-center">
              <div className="flex h-14 w-14 items-center justify-center rounded-full bg-muted mb-4 shadow-sm border border-border/50 ring-4 ring-muted/20">
                <FolderOpen className="h-7 w-7 text-muted-foreground/70" />
              </div>
              <p className="text-[15px] font-semibold text-foreground mb-1.5">
                {t('settings.projectsEmpty')}
              </p>
              <p className="text-xs text-muted-foreground max-w-[240px]">
                {t('settings.projectsEmptyHint')}
              </p>
            </div>
          ) : (
            <CardContent className="p-0 divide-y divide-border/40">
              {projects.map((path) => (
                <ProjectRow
                  key={path}
                  path={path}
                  onRemove={(path) => removeProject(path)}
                />
              ))}
            </CardContent>
          )}
        </Card>
      </section>
    </div>
  );
}
