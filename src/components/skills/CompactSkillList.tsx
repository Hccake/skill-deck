// src/components/skills/CompactSkillList.tsx
import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { ScrollArea } from '@/components/ui/scroll-area';
import { CompactSkillItem } from './CompactSkillItem';
import type { InstalledSkill, SkillScope } from '@/bindings';

interface CompactSkillListProps {
  globalSkills: InstalledSkill[];
  projectSkills: InstalledSkill[];
  selectedSkillName: string | null;
  selectedSkillScope: SkillScope | null;
  isProjectSelected: boolean;
  projectTitle: string;
  onSkillClick: (skill: InstalledSkill) => void;
}

export const CompactSkillList = memo(function CompactSkillList({
  globalSkills,
  projectSkills,
  selectedSkillName,
  selectedSkillScope,
  isProjectSelected,
  projectTitle,
  onSkillClick,
}: CompactSkillListProps) {
  const { t } = useTranslation();

  return (
    <div className="flex-1 relative min-h-0">
      <ScrollArea className="absolute inset-0 w-full h-full">
        <div className="p-2 w-full overflow-hidden">
          {/* Project skills section */}
          {isProjectSelected && projectSkills.length > 0 ? (
            <div className="mb-3">
              <div className="text-[11px] font-semibold text-muted-foreground uppercase tracking-wider px-1.5 mb-1">
                {projectTitle} · {projectSkills.length}
              </div>
              {projectSkills.map((skill) => (
                <CompactSkillItem
                  key={`project:${skill.name}`}
                  skill={skill}
                  isSelected={selectedSkillName === skill.name && selectedSkillScope === 'project'}
                  onClick={onSkillClick}
                />
              ))}
            </div>
          ) : null}

          {/* Global skills section */}
          {globalSkills.length > 0 ? (
            <div>
              <div className="text-[11px] font-semibold text-muted-foreground uppercase tracking-wider px-1.5 mb-1">
                {t('skills.globalSkills')} · {globalSkills.length}
              </div>
              {globalSkills.map((skill) => (
                <CompactSkillItem
                  key={`global:${skill.name}`}
                  skill={skill}
                  isSelected={selectedSkillName === skill.name && selectedSkillScope === 'global'}
                  onClick={onSkillClick}
                />
              ))}
            </div>
          ) : null}
        </div>
      </ScrollArea>
    </div>
  );
});
