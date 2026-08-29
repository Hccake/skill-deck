import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { LibraryBig, Pencil, Plus, Trash2 } from 'lucide-react';
import { toast } from 'sonner';
import { cn } from '@/lib/utils';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { summarizeLibraryUsage } from '@/lib/libraries/usage-presentation';
import type { LibraryId, LibraryUsageProjection, SkillLibrarySummary } from '@/bindings';

interface LibrarySidebarProps {
  libraries: SkillLibrarySummary[];
  usageProjection?: readonly LibraryUsageProjection[];
  selectedLibraryId: LibraryId | null;
  busy?: boolean;
  onSelectLibrary: (libraryId: LibraryId) => void;
  onCreateLibrary: () => void;
  onRenameLibrary: (library: SkillLibrarySummary) => void;
  onDeleteLibrary: (library: SkillLibrarySummary) => void;
}

interface LibrarySidebarItemProps {
  library: SkillLibrarySummary;
  usageProjection?: readonly LibraryUsageProjection[];
  isSelected: boolean;
  busy: boolean;
  onSelect: () => void;
  onRename: () => void;
  onDelete: () => void;
}

/**
 * 副行只表达"是否生效"，不列位置名。
 *
 * 侧栏服务的决策是"进哪个库"，位置名在这个宽度下必然截断成没有信息量的残句；具体生效在哪
 * 由主区 header 回答。计数固定宽度，各行形状一致可以纵向比较。
 */
function usageLabel(
  library: SkillLibrarySummary,
  usageProjection: readonly LibraryUsageProjection[] | undefined,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  const usage = summarizeLibraryUsage(usageProjection, library.id);
  if (usage.applied && usage.pendingAdjustment) {
    return t('libraries.usage.appliedWithPending', { count: usage.confirmedCount });
  }
  if (usage.applied) return t('libraries.usage.applied', { count: usage.confirmedCount });
  if (usage.pendingAdjustment) return t('libraries.usage.pendingOnly');
  return t('libraries.usage.unapplied');
}

function LibrarySidebarItem({
  library,
  usageProjection,
  isSelected,
  busy,
  onSelect,
  onRename,
  onDelete,
}: LibrarySidebarItemProps) {
  const { t } = useTranslation();
  const usage = summarizeLibraryUsage(usageProjection, library.id);
  // 已确认生效与未完成调整都会锁定成员，与后端 `usages()` 的并集语义一致。
  const locked = usage.applied || usage.pendingAdjustment;
  const deleteLockedReason = usage.applied && usage.pendingAdjustment
    ? t('libraries.lockedDeleteAppliedWithPending')
    : usage.pendingAdjustment
      ? t('libraries.lockedDeletePending')
      : usage.applied
        ? t('libraries.lockedDeleteApplied')
        : null;

  const handleDelete = () => {
    if (deleteLockedReason) {
      toast.info(deleteLockedReason);
      return;
    }
    onDelete();
  };

  return (
    <div
      data-library-id={library.id}
      className={cn(
        'library-sidebar-item group relative flex w-full transition-colors',
        isSelected
          ? 'bg-primary/10 text-primary shadow-[inset_2px_0_0_0_theme(colors.primary.DEFAULT)]'
          : 'text-muted-foreground hover:bg-foreground/[0.04] hover:text-foreground',
      )}
    >
      <button
        type="button"
        onClick={onSelect}
        className="flex min-w-0 flex-1 items-center gap-3 px-4 py-2 pr-16 text-left cursor-pointer"
      >
        <LibraryBig className={cn('h-4 w-4 flex-shrink-0', isSelected ? 'text-primary' : 'text-muted-foreground')} aria-hidden="true" />
        <span className="min-w-0 flex-1">
          <span className={cn('block truncate text-sm', isSelected ? 'font-bold' : 'font-medium')}>
            {library.name}
          </span>
          <span className={cn('block truncate text-[10px]', isSelected ? 'opacity-70' : 'opacity-60')}>
            {usageLabel(library, usageProjection, t)}
          </span>
        </span>
      </button>
      <div className="absolute right-3 top-1/2 flex -translate-y-1/2 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100">
        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6 hover:bg-background/80 cursor-pointer"
          onClick={onRename}
          aria-label={t('libraries.renameNamed', { name: library.name })}
          title={t('libraries.rename')}
          disabled={busy}
        >
          <Pencil className="h-3.5 w-3.5" />
        </Button>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className={cn(
                'h-6 w-6',
                locked
                  ? 'cursor-not-allowed text-muted-foreground/40 hover:bg-transparent hover:text-muted-foreground/40'
                  : 'cursor-pointer hover:bg-destructive/10 hover:text-destructive',
              )}
              onClick={handleDelete}
              aria-label={t('libraries.deleteNamed', { name: library.name })}
              aria-disabled={locked || undefined}
              disabled={busy}
            >
              <Trash2 className="h-3.5 w-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="top" sideOffset={4} className="max-w-72">
            {deleteLockedReason ?? t('libraries.deleteLibrary')}
          </TooltipContent>
        </Tooltip>
      </div>
    </div>
  );
}

export const LibrarySidebar = memo(function LibrarySidebar({
  libraries,
  usageProjection,
  selectedLibraryId,
  busy = false,
  onSelectLibrary,
  onCreateLibrary,
  onRenameLibrary,
  onDeleteLibrary,
}: LibrarySidebarProps) {
  const { t } = useTranslation();

  return (
    <aside className="libraries-sidebar flex h-full min-h-0 flex-col flex-shrink-0 border-r border-border/50 bg-canvas">
      <div className="flex min-h-0 flex-1 flex-col pt-5">
        <h3 className="flex-shrink-0 px-4 mb-2 text-xs font-semibold text-muted-foreground/80">
          {t('libraries.title')}
        </h3>
        <div data-testid="libraries-sidebar-scroll" className="min-h-0 flex-1 overflow-y-auto">
          {libraries.length === 0 ? (
            <p className="px-4 py-2 text-xs text-muted-foreground">{t('libraries.noLibraries')}</p>
          ) : (
            <div className="space-y-0.5">
              {libraries.map((library) => (
                <LibrarySidebarItem
                  key={library.id}
                  library={library}
                  usageProjection={usageProjection}
                  isSelected={selectedLibraryId === library.id}
                  busy={busy}
                  onSelect={() => onSelectLibrary(library.id)}
                  onRename={() => onRenameLibrary(library)}
                  onDelete={() => onDeleteLibrary(library)}
                />
              ))}
            </div>
          )}
        </div>
      </div>

      <div className="p-4">
        <button
          type="button"
          className="w-full flex items-center justify-start gap-1.5 px-3 py-2 rounded-md hover:bg-foreground/[0.04] transition-colors text-muted-foreground hover:text-foreground font-semibold text-sm cursor-pointer"
          onClick={onCreateLibrary}
          aria-label={t('libraries.create')}
          disabled={busy}
        >
          <Plus className="h-4 w-4" />
          {t('libraries.create')}
        </button>
      </div>
    </aside>
  );
});
