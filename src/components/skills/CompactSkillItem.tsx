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
          ? 'bg-primary/[0.06] border-y border-primary/15 select-none'
          : 'hover:bg-accent/50 text-muted-foreground'
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
        'w-full text-sm truncate mt-0.5 leading-relaxed',
        isSelected ? 'text-primary/60' : 'text-muted-foreground'
      )}>
        {skill.description}
      </div>
    </button>
  );
});
