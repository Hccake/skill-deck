// src/components/skills/CompactSkillItem.tsx
import { memo, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import type { InstalledSkill } from '@/bindings';
import { skillStatusPresentation } from '@/lib/skill-status-presentation';
import { SkillCompactRow } from './card/SkillCardPrimitives';

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
  const { t } = useTranslation();
  const handleClick = useCallback(() => {
    onClick(skill);
  }, [onClick, skill]);
  const presentation = skillStatusPresentation(skill);
  const source = presentation.local ? t('skills.updateStatusLabel.localSource')
    : presentation.sourceLabel ?? (presentation.notice ? t(presentation.notice.labelKey) : null);
  return <SkillCompactRow name={skill.name} source={source} selected={isSelected} onClick={handleClick} />;
});
