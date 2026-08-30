import { memo, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus, SlidersHorizontal } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import {
  getSkillIdentity,
  getSkillIdentityKey,
  isSameSkillIdentity,
  type SkillIdentity,
} from '@/lib/skills/identity';
import { CompactSkillItem } from './CompactSkillItem';
import { LibraryApplicationStrip } from './LibraryApplicationStrip';
import type { InstalledSkill, LibraryApplicationSummary } from '@/bindings';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';

interface CompactSkillListProps {
  globalSkills: InstalledSkill[];
  projectSkills: InstalledSkill[];
  selectedSkillRef: SkillIdentity | null;
  isProjectSelected: boolean;
  projectTitle: string;
  projectPath?: string;
  pathExists?: boolean;
  onAddProject?: () => void;
  onAddGlobal?: () => void;
  onSkillClick: (skill: InstalledSkill) => void;
  projectEmptyState?: ReactNode;
  globalEmptyState?: ReactNode;
  projectLibraryApplication?: LibraryApplicationSummary;
  globalLibraryApplication?: LibraryApplicationSummary;
  onManageProjectLibraries?: () => void;
  onManageGlobalLibraries?: () => void;
}

export const CompactSkillList = memo(function CompactSkillList({
  globalSkills,
  projectSkills,
  selectedSkillRef,
  isProjectSelected,
  projectTitle,
  projectPath,
  pathExists = true,
  onAddProject,
  onAddGlobal,
  onSkillClick,
  projectEmptyState,
  globalEmptyState,
  projectLibraryApplication,
  globalLibraryApplication,
  onManageProjectLibraries,
  onManageGlobalLibraries,
}: CompactSkillListProps) {
  const { t } = useTranslation();
  const writeBlocked = useBusinessWriteBlocked();

  return (
    <div className="flex-1 relative min-h-0">
      <ScrollArea className="absolute inset-0 w-full h-full">
        <div className="pt-2 w-full overflow-hidden">
          {/* Project skills section */}
          {isProjectSelected ? (
            <div className="mb-4">
              <div className="flex items-center justify-between px-1.5 mb-1.5 mt-1">
                <div className="text-xs font-semibold uppercase tracking-wider text-muted-foreground flex items-center gap-1.5">
                  {projectTitle}
                  <span className="font-normal opacity-70">({projectSkills.length})</span>
                </div>
                <div className="flex items-center gap-0.5">
                  {pathExists && onManageProjectLibraries ? (
                    <Button variant="ghost" size="icon" className="size-6 text-muted-foreground hover:bg-primary/10 hover:text-primary" onClick={onManageProjectLibraries} aria-label={t('libraries.manage')} title={t('libraries.manage')}>
                      <SlidersHorizontal className="size-3.5" aria-hidden="true" />
                    </Button>
                  ) : null}
                  {pathExists && onAddProject ? (
                    <Button variant="ghost" size="icon" className="h-5 w-5 text-muted-foreground hover:text-primary hover:bg-primary/10 cursor-pointer rounded-md transition-colors" onClick={onAddProject} aria-label={t('skills.add')} title={t('skills.add')} disabled={writeBlocked}>
                      <Plus className="h-3.5 w-3.5" aria-hidden="true" />
                    </Button>
                  ) : null}
                </div>
              </div>
              {pathExists && projectLibraryApplication ? <LibraryApplicationStrip application={projectLibraryApplication} compact /> : null}
              {projectSkills.length > 0
                ? projectSkills.map((skill) => (
                    <CompactSkillItem
                      key={getSkillIdentityKey(getSkillIdentity(skill, projectPath))}
                      skill={skill}
                      isSelected={isSameSkillIdentity(selectedSkillRef, getSkillIdentity(skill, projectPath))}
                      onClick={onSkillClick}
                    />
                  ))
                : projectEmptyState}
            </div>
          ) : null}

          {/* Global skills section */}
          <div className="mb-4">
            <div className="flex items-center justify-between px-1.5 mb-1.5 mt-1">
              <div className="text-xs font-semibold uppercase tracking-wider text-muted-foreground flex items-center gap-1.5">
                {t('skills.globalSkills')}
                <span className="font-normal opacity-70">({globalSkills.length})</span>
              </div>
              <div className="flex items-center gap-0.5">
                {onManageGlobalLibraries ? (
                  <Button variant="ghost" size="icon" className="size-6 text-muted-foreground hover:bg-primary/10 hover:text-primary" onClick={onManageGlobalLibraries} aria-label={t('libraries.manage')} title={t('libraries.manage')}>
                    <SlidersHorizontal className="size-3.5" aria-hidden="true" />
                  </Button>
                ) : null}
                {onAddGlobal ? (
                  <Button variant="ghost" size="icon" className="h-5 w-5 text-muted-foreground hover:text-primary hover:bg-primary/10 cursor-pointer rounded-md transition-colors" onClick={onAddGlobal} aria-label={t('skills.add')} title={t('skills.add')} disabled={writeBlocked}>
                    <Plus className="h-3.5 w-3.5" aria-hidden="true" />
                  </Button>
                ) : null}
              </div>
            </div>
            {globalLibraryApplication ? <LibraryApplicationStrip application={globalLibraryApplication} compact /> : null}
            {globalSkills.length > 0
              ? globalSkills.map((skill) => (
                  <CompactSkillItem
                    key={getSkillIdentityKey(getSkillIdentity(skill))}
                    skill={skill}
                    isSelected={isSameSkillIdentity(selectedSkillRef, getSkillIdentity(skill))}
                    onClick={onSkillClick}
                  />
                ))
              : globalEmptyState}
          </div>
        </div>
      </ScrollArea>
    </div>
  );
});
