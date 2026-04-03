// src/components/skills/CompactSkillItem.tsx
import { memo, useCallback } from 'react';
import type { InstalledSkill } from '@/bindings';
import { cn } from '@/lib/utils';

interface CompactSkillItemProps {
  skill: InstalledSkill;
  isSelected: boolean;
  onClick: (skill: InstalledSkill) => void;
}

export const CompactSkillItem = memo(function CompactSkillItem({
  skill,
  isSelected,
  onClick,
}: CompactSkillItemProps) {
  const handleClick = useCallback(() => {
    onClick(skill);
  }, [onClick, skill]);

  return (
    <button
      type="button"
      className={cn(
        'group relative w-full text-left px-4 py-2.5 block overflow-hidden transition-colors cursor-pointer',
        isSelected
          ? 'bg-primary/5 select-none shadow-[inset_2px_0_0_0_theme(colors.primary.DEFAULT)]'
          : 'hover:bg-foreground/[0.03] text-muted-foreground'
      )}
      onClick={handleClick}
    >
      <div className={cn(
        'w-full text-sm tracking-tight truncate',
        isSelected ? 'font-heading font-bold text-primary' : 'font-heading font-semibold text-foreground'
      )}>
        {skill.name}
      </div>
      <div className={cn(
        'w-full text-xs truncate mt-0.5 flex items-center gap-1',
        isSelected ? 'text-primary/70' : 'text-muted-foreground/60'
      )}>
        {skill.source ? (
          <span className="font-mono">{skill.source}</span>
        ) : (
          <span className="italic opacity-60">Local</span>
        )}
      </div>
    </button>
  );
});
