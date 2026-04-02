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
        'group relative w-full text-left rounded-md px-3 py-2 block overflow-hidden transition-all duration-200 cursor-pointer border border-transparent',
        isSelected
          ? 'bg-primary/10 select-none'
          : 'hover:bg-accent/50 text-muted-foreground'
      )}
      onClick={handleClick}
    >
      {isSelected && (
        <div className="absolute left-0 top-1.5 bottom-1.5 w-[3px] bg-primary rounded-r-md" />
      )}
      <div className={cn(
        'w-full text-sm font-medium tracking-tight truncate',
        isSelected ? 'text-primary' : 'text-foreground/80'
      )}>
        {skill.name}
      </div>
      <div className="w-full text-sm text-foreground/60 truncate mt-0.5 leading-relaxed">
        {skill.description}
      </div>
    </button>
  );
});
