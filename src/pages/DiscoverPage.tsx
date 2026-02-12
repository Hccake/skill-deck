import { useMemo, useCallback } from 'react';
import { useSkillsStore } from '@/stores/skills';
import { SkillSearch } from '@/components/skills/skill-search';
import type { SearchSkill } from '@/components/skills/skill-search';

export function DiscoverPage() {
  const globalSkills = useSkillsStore((s) => s.globalSkills);
  const projectSkills = useSkillsStore((s) => s.projectSkills);
  const openAddWithPrefill = useSkillsStore((s) => s.openAddWithPrefill);

  const installedSkillKeys = useMemo(() => {
    const keys = new Set<string>();
    for (const s of globalSkills) keys.add(`${s.source ?? ''}::${s.name}`);
    for (const s of projectSkills) keys.add(`${s.source ?? ''}::${s.name}`);
    return keys;
  }, [globalSkills, projectSkills]);

  const handleInstall = useCallback((skill: SearchSkill) => {
    openAddWithPrefill({
      source: skill.source,
      skillName: skill.name,
    });
  }, [openAddWithPrefill]);

  return (
    <div className="flex-1 flex flex-col overflow-hidden px-4 sm:px-6 py-4 sm:py-5">
      <div className="mx-auto w-full max-w-xl lg:max-w-2xl">
        <SkillSearch
          installedSkillKeys={installedSkillKeys}
          onInstall={handleInstall}
        />
      </div>
    </div>
  );
}
