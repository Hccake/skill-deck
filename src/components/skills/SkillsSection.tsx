// src/components/skills/SkillsSection.tsx
import { memo, useState, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus, AlertTriangle, Check } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { SkillCard } from './SkillCard';
import type { AgentType, InstalledSkill, SkillAuditData, SkillScope } from '@/bindings';

// 提升默认值避免重复创建 — rerender-memo-with-default-value 规则
const EMPTY_CONFLICT_SET = new Set<string>();
const EMPTY_DISPLAY_NAMES = new Map<AgentType, string>();
const EMPTY_AUDIT_CACHE: Record<string, SkillAuditData> = {};

interface SkillsSectionProps {
  title: string;
  skills: InstalledSkill[];
  scope: SkillScope;
  conflictSkillNames?: Set<string>;
  /** 项目目录是否存在（仅 project scope） */
  pathExists?: boolean;
  /** 项目路径（仅 project scope，用于提示信息） */
  projectPath?: string;
  /** 各 skill 的更新状态 */
  updatingSkills: Map<string, 'queued' | 'updating' | 'done' | 'failed'>;
  /** 是否正在检查更新 */
  isCheckingUpdates?: boolean;
  /** Agent display name 映射（agentId → displayName） */
  agentDisplayNames?: Map<AgentType, string>;
  /** 审计数据缓存（skillName → SkillAuditData） */
  auditCache?: Record<string, SkillAuditData>;
  onSkillClick: (skill: InstalledSkill) => void;
  onUpdate: (skillName: string, scope: SkillScope) => Promise<void>;
  onUpdateAll: (scope: SkillScope) => Promise<void>;
  onCancelUpdateAll: () => void;
  onDelete: (skill: InstalledSkill) => void;
  onAdd: () => void;
  onCheckUpdates?: () => void;
  emptyState?: React.ReactNode;
}

export const SkillsSection = memo(function SkillsSection({
  title,
  skills,
  scope,
  conflictSkillNames = EMPTY_CONFLICT_SET,
  pathExists = true,
  projectPath,
  updatingSkills,
  isCheckingUpdates = false,
  agentDisplayNames = EMPTY_DISPLAY_NAMES,
  auditCache = EMPTY_AUDIT_CACHE,
  onSkillClick,
  onUpdate,
  onUpdateAll,
  onCancelUpdateAll,
  onDelete,
  onAdd,
  onCheckUpdates,
  emptyState,
}: SkillsSectionProps) {
  const { t } = useTranslation();

  // 单次遍历派生所有更新相关状态（js-combine-iterations）— 仅统计当前 section 的 skills
  let updatesCount = 0;
  let isAnyUpdating = false;
  let completedCount = 0;
  let totalUpdating = 0;
  for (const skill of skills) {
    if (skill.hasUpdate) updatesCount++;
    const status = updatingSkills.get(skill.name);
    if (status) {
      totalUpdating++;
      if (status === 'queued' || status === 'updating') {
        isAnyUpdating = true;
      } else {
        completedCount++;
      }
    }
  }

  // 检测 isCheckingUpdates true → false 转换，短暂显示完成态
  const [checkDone, setCheckDone] = useState(false);
  const wasCheckingRef = useRef(false);
  useEffect(() => {
    if (isCheckingUpdates) {
      wasCheckingRef.current = true;
    } else if (wasCheckingRef.current) {
      wasCheckingRef.current = false;
      // 有更新时跳过 ✓ 反馈（已有 "X updates" 信号）
      if (updatesCount === 0) {
        setCheckDone(true);
        const timer = setTimeout(() => setCheckDone(false), 800);
        return () => clearTimeout(timer);
      }
    }
  }, [isCheckingUpdates, updatesCount]);

  return (
    <section className="mb-6">
      {/* Section Header */}
      <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2 mb-3">
        <div className="flex items-center gap-2 flex-wrap">
          <h2 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground flex items-center gap-1.5">
            {title}
            <span className="font-normal opacity-70">({skills.length})</span>
          </h2>
          {isCheckingUpdates && updatesCount === 0 && (
            <>
              <span className="text-muted-foreground/50">·</span>
              <span className="text-xs text-muted-foreground animate-pulse">
                {t('skills.checking')}
              </span>
            </>
          )}

          {isAnyUpdating ? (
            <>
              <span className="text-muted-foreground/50">·</span>
              <span className="text-xs font-medium text-warning">
                {t('skills.updateAllProgress', { completed: completedCount, total: totalUpdating })}
              </span>
              <Button variant="ghost" size="sm" className="h-5 px-1.5 text-xs text-muted-foreground cursor-pointer"
                onClick={() => onCancelUpdateAll()}>
                {t('skills.cancel')}
              </Button>
            </>
          ) : updatesCount > 0 ? (
            <>
              <span className="text-muted-foreground/50">·</span>
              <span className="text-xs font-medium text-warning">
                {updatesCount} {t(updatesCount === 1 ? 'skills.update' : 'skills.updates')}
              </span>
              <Button variant="outline" size="sm" className="h-5 px-1.5 text-xs cursor-pointer"
                onClick={() => onUpdateAll(scope)}>
                {t('skills.updateAll')}
              </Button>
            </>
          ) : null}
          {/* Check 按钮：不在 batch 更新中、有 skills 时始终显示 */}
          {!isAnyUpdating && onCheckUpdates && skills.length > 0 && (
            checkDone ? (
              <span className="inline-flex items-center gap-1 h-5 px-1.5 text-xs text-success font-medium">
                <Check className="h-3 w-3" />
              </span>
            ) : (
              <Button variant="ghost" size="sm" className="h-5 px-1.5 text-xs text-muted-foreground cursor-pointer"
                disabled={isCheckingUpdates}
                onClick={onCheckUpdates}>
                {t('skills.checkUpdates')}
              </Button>
            )
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
              {skills.map((skill) => {
                const updateStatus = updatingSkills.get(skill.name);
                return (
                  <SkillCard
                    key={skill.name}
                    skill={skill}
                    displayScope={scope}
                    hasConflict={conflictSkillNames.has(skill.name)}
                    updateStatus={updateStatus}
                    agentDisplayNames={agentDisplayNames}
                    riskLevel={auditCache[skill.name]?.risk}
                    onClick={onSkillClick}
                    onUpdate={(name) => onUpdate(name, scope)}
                    onDelete={onDelete}
                  />
                );
              })}
            </div>
          )}
        </>
      )}
    </section>
  );
});
