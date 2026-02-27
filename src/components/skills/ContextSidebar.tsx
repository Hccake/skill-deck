// src/components/skills/ContextSidebar.tsx
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
import { useContextStore } from '@/stores/context';
import { openInExplorer } from '@/hooks/useTauriApi';
import { cn } from '@/lib/utils';

/** 从完整路径中提取项目名称（最后一个目录名） */
function getProjectName(path: string): string {
  const parts = path.replace(/\\/g, '/').split('/');
  return parts[parts.length - 1] || path;
}

/** Global Context Item 组件 */
function GlobalContextItem() {
  const { t } = useTranslation();
  const { selectedContext, selectContext } = useContextStore();
  const isSelected = selectedContext === 'global';

  return (
    <button
      onClick={() => selectContext('global')}
      className={cn(
        'w-full rounded-md px-3 py-2 text-left transition-colors duration-200',
        'cursor-pointer',
        isSelected
          ? 'bg-foreground/[0.06] text-foreground font-medium'
          : 'text-muted-foreground hover:bg-foreground/[0.03] hover:text-foreground'
      )}
    >
      <div className="flex items-center gap-2.5">
        <Globe className="h-[18px] w-[18px] text-primary flex-shrink-0" />
        <div className="min-w-0">
          <span className="text-sm">{t('context.global')}</span>
          <p className="text-xs text-muted-foreground/60 truncate mt-0.5">{t('context.globalSubtitle')}</p>
        </div>
      </div>
    </button>
  );
}

/** Project Context Item 组件 */
function ProjectContextItem({ project }: { project: string }) {
  const { t } = useTranslation();
  const { selectedContext, toggleProjectContext, removeProject } = useContextStore();
  const isSelected = selectedContext === project;
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);

  const projectName = getProjectName(project);

  const handleOpenInExplorer = async (e?: React.MouseEvent) => {
    e?.stopPropagation();
    try {
      await openInExplorer(project);
    } catch (error) {
      console.error('Failed to open in explorer:', error);
    }
  };

  const handleRemove = async () => {
    await removeProject(project);
    setDeleteDialogOpen(false);
  };

  const itemButton = (
    <div
      role="button"
      tabIndex={0}
      onClick={() => toggleProjectContext(project)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          toggleProjectContext(project);
        }
      }}
      className={cn(
        'w-full rounded-md px-3 py-2 text-left transition-colors duration-200',
        'group relative cursor-pointer',
        isSelected
          ? 'bg-foreground/[0.06] text-foreground font-medium'
          : 'text-muted-foreground hover:bg-foreground/[0.03] hover:text-foreground'
      )}
    >
      <div className="flex items-center gap-2.5">
        <Folder className={cn('h-[18px] w-[18px] flex-shrink-0', isSelected ? 'text-foreground' : 'text-muted-foreground')} />

        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className="text-sm truncate">{projectName}</span>

            {/* Hover 时显示的操作按钮 */}
            <div className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity duration-200 ml-auto">
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
                onClick={(e) => {
                  e.stopPropagation();
                  setDeleteDialogOpen(true);
                }}
                aria-label={t('context.remove')}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>

          {/* 项目路径 */}
          <p className="text-xs text-muted-foreground/60 truncate mt-0.5">
            {project}
          </p>
        </div>
      </div>
    </div>
  );

  return (
    <>
      {/* 右键菜单支持 */}
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
          >
            <Trash2 className="h-4 w-4 mr-2" />
            {t('context.remove')}
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>

      {/* 删除确认对话框 */}
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
              onClick={(e) => {
                e.preventDefault();
                handleRemove();
              }}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90 cursor-pointer"
            >
              {t('context.removeConfirm.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}

/** ContextSidebar 主组件 */
export function ContextSidebar() {
  const { t } = useTranslation();
  const { projects, projectsLoaded, loadProjects, addProject } = useContextStore();

  // 初始化加载 projects
  useEffect(() => {
    if (!projectsLoaded) {
      loadProjects();
    }
  }, [projectsLoaded, loadProjects]);

  const handleAddProject = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: t('context.addProject'),
      });
      if (selected && typeof selected === 'string') {
        await addProject(selected);
      }
    } catch (error) {
      console.error('Failed to open folder picker:', error);
    }
  };

  return (
    <aside className="w-60 flex-shrink-0 border-r border-border flex flex-col h-full">
      {/* Global */}
      <div className="px-4 pt-4 sm:pt-5">
        <GlobalContextItem />
      </div>

      {/* Projects 列表区域（可滚动） */}
      <div className="flex-1 overflow-auto">
        <div className="px-4">
          {projects.length === 0 ? (
            <p className="py-2 text-xs text-muted-foreground">
              {t('context.noProjects')}
            </p>
          ) : (
            <div className="space-y-1">
              {projects.map((project) => (
                <ProjectContextItem key={project} project={project} />
              ))}
            </div>
          )}

          {/* Add Project Button */}
          <Button
            variant="ghost"
            size="sm"
            className="w-full justify-start gap-2.5 mt-2 text-muted-foreground hover:bg-foreground/[0.03] hover:text-foreground cursor-pointer"
            onClick={handleAddProject}
          >
            <Plus className="h-[18px] w-[18px]" />
            {t('context.addProject')}
          </Button>
        </div>
      </div>

    </aside>
  );
}
