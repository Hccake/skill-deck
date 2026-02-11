// src/components/skills/SkillsSection.tsx
import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus, AlertTriangle } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { SkillCard } from './SkillCard';
import type { AgentType, Skill, SkillScope } from '@/types';

// 提升默认值避免重复创建 — rerender-memo-with-default-value 规则
const EMPTY_CONFLICT_SET = new Set<string>();
const EMPTY_DISPLAY_NAMES = new Map<AgentType, string>();

interface SkillsSectionProps {
  title: string;
  skills: Skill[];
  scope: SkillScope;
  conflictSkillNames?: Set<string>;
  /** 项目目录是否存在（仅 project scope） */
  pathExists?: boolean;
  /** 项目路径（仅 project scope，用于提示信息） */
  projectPath?: string;
  /** 当前正在更新的 skill 名称 */
  updatingSkill?: string | null;
  /** Agent display name 映射（agentId → displayName） */
  agentDisplayNames?: Map<AgentType, string>;
  onSkillClick: (skill: Skill) => void;
  onUpdate: (skillName: string) => void;
  onDelete: (skillName: string) => void;
  onToggleAgent: (skillName: string, agentId: string) => void;
  onAdd: () => void;
  emptyState?: React.ReactNode;
}

export function SkillsSection({
  title,
  skills,
  scope,
  conflictSkillNames = EMPTY_CONFLICT_SET,
  pathExists = true,
  projectPath,
  updatingSkill,
  agentDisplayNames = EMPTY_DISPLAY_NAMES,
  onSkillClick,
  onUpdate,
  onDelete,
  onToggleAgent,
  onAdd,
  emptyState,
}: SkillsSectionProps) {
  const { t } = useTranslation();
  const updatesCount = useMemo(() => skills.filter((s) => s.hasUpdate).length, [skills]);

  return (
    <section className="mb-6">
      {/* Section Header */}
      <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2 mb-3">
        <div className="flex items-center gap-2 flex-wrap">
          <h2 className="text-sm font-semibold text-foreground">
            {title} ({skills.length})
          </h2>
          {updatesCount > 0 && (
            <>
              <span className="text-muted-foreground/50">·</span>
              <span className="text-xs font-medium text-warning">
                {updatesCount} {t(updatesCount === 1 ? 'skills.update' : 'skills.updates')}
              </span>
            </>
          )}
        </div>
        {/* 路径不存在时隐藏 Add 按钮 */}
        {pathExists && (
          <Button
            size="sm"
            className="h-8 px-2 sm:px-3 text-xs gap-1 sm:gap-1.5 shadow-sm cursor-pointer"
            onClick={onAdd}
          >
            <Plus className="h-3.5 w-3.5" />
            {t('skills.add')}
          </Button>
        )}
      </div>

      {/* 路径不存在提示 */}
      {!pathExists && (
        <div className="flex items-center gap-2 p-3 mb-3 bg-amber-500/10 text-amber-700 dark:text-amber-400 text-sm rounded-md border border-amber-500/20">
          <AlertTriangle className="h-4 w-4 shrink-0" />
          <span>
            {t('skills.projectNotFound', { path: projectPath })}
          </span>
        </div>
      )}

      {/* Skills List */}
      {pathExists && (
        <>
          {skills.length === 0 ? (
            emptyState
          ) : (
            <div className="grid gap-3">
              {skills.map((skill) => (
                <SkillCard
                  key={skill.name}
                  skill={skill}
                  displayScope={scope}
                  hasConflict={conflictSkillNames.has(skill.name)}
                  isUpdating={updatingSkill === skill.name}
                  agentDisplayNames={agentDisplayNames}
                  onClick={onSkillClick}
                  onUpdate={onUpdate}
                  onDelete={onDelete}
                  onToggleAgent={onToggleAgent}
                />
              ))}
            </div>
          )}
        </>
      )}
    </section>
  );
}
