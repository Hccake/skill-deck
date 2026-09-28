import { memo, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { skillStatusPresentation } from '@/lib/skill-status-presentation';
import { SkillCompactRow } from '@/components/skills/card/SkillCardPrimitives';
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
 * 与 Skills 共用行的呈现，成员选择继续由库页面处理。
 */
export const LibraryCompactItem = memo(function LibraryCompactItem({
  skill,
  isSelected,
  onClick,
}: LibraryCompactItemProps) {
  const { t } = useTranslation();
  const handleClick = useCallback(() => onClick(skill.name), [onClick, skill.name]);
  const presentation = skillStatusPresentation(skill);
  const source = presentation.local ? t('skills.updateStatusLabel.localSource')
    : presentation.sourceLabel ?? (presentation.notice ? t(presentation.notice.labelKey) : null);
  return <SkillCompactRow name={skill.name} source={source} selected={isSelected} onClick={handleClick} />;
});
