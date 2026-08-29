import { memo, useCallback } from 'react';
import { cn } from '@/lib/utils';
import type { LibrarySkillSummary } from '@/bindings';

interface LibraryCompactItemProps {
  skill: LibrarySkillSummary;
  isSelected: boolean;
  onClick: (skillName: string) => void;
}

/**
 * 分栏视图下的库成员列表行。
 *
 * 选中成员后列表退居为导航：操作和完整信息都归详情面板，这里只保留辨认所需的名称与来源。
 * 形态与 `Skills` 页的 `CompactSkillItem` 保持一致，但各自维护——共享面只有标记，没有行为。
 */
export const LibraryCompactItem = memo(function LibraryCompactItem({
  skill,
  isSelected,
  onClick,
}: LibraryCompactItemProps) {
  const handleClick = useCallback(() => onClick(skill.name), [onClick, skill.name]);
  const subtitle = skill.source?.trim() || skill.sourceUrl?.trim() || null;

  return (
    <button
      type="button"
      className={cn(
        'group relative block w-full cursor-pointer overflow-hidden px-4 py-2.5 text-left transition-colors',
        isSelected
          ? 'bg-primary/5 select-none shadow-[inset_2px_0_0_0_theme(colors.primary.DEFAULT)]'
          : 'text-muted-foreground hover:bg-foreground/[0.03]',
      )}
      onClick={handleClick}
    >
      <div className={cn(
        'w-full truncate text-sm tracking-tight',
        isSelected ? 'font-heading font-bold text-primary' : 'font-heading font-semibold text-foreground',
      )}>
        {skill.name}
      </div>
      {subtitle ? (
        <div className={cn(
          'mt-0.5 w-full truncate font-mono text-xs',
          isSelected ? 'text-primary/70' : 'text-muted-foreground/60',
        )}>
          {subtitle}
        </div>
      ) : null}
    </button>
  );
});
