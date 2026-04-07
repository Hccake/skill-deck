// src/components/skills/SkillsSection.tsx
import { memo, useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus, AlertTriangle, Check } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { SkillCard } from './SkillCard';
import { getSkillIdentityKey } from '@/lib/skills/identity';
import { cn } from '@/lib/utils';
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
  onCopyToProject?: (skill: InstalledSkill) => void;
  onManageAgents?: (skill: InstalledSkill) => void;
  onAdd: () => void;
  onCheckUpdates?: () => Promise<boolean>;
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
  onCopyToProject,
  onManageAgents,
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
    const status = updatingSkills.get(
      getSkillIdentityKey({ name: skill.name, scope: skill.scope, projectPath })
    );
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
  const hideCheckDoneTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (hideCheckDoneTimerRef.current) {
        clearTimeout(hideCheckDoneTimerRef.current);
      }
    };
  }, []);

  const showCheckDone = useCallback(() => {
    if (hideCheckDoneTimerRef.current) {
      clearTimeout(hideCheckDoneTimerRef.current);
    }
    setCheckDone(true);
    hideCheckDoneTimerRef.current = setTimeout(() => {
      setCheckDone(false);
      hideCheckDoneTimerRef.current = null;
    }, 800);
  }, []);

  const handleCheckUpdates = useCallback(async () => {
    if (!onCheckUpdates || isCheckingUpdates) return;
    const succeeded = await onCheckUpdates();
    if (!succeeded) return;
    showCheckDone();
  }, [isCheckingUpdates, onCheckUpdates, showCheckDone]);

  return (
    <section className="mb-6">
      {/* Section Header */}
      <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2 mb-3">
        <div className="flex items-center gap-2 flex-wrap">
          <h2 className="text-sm font-bold tracking-tight text-foreground/90 flex items-center gap-1.5">
            {title}
            <span className="text-xs font-semibold opacity-50">({skills.length})</span>
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
        </div>
        
        {/* Right Actions: Check Updates + Add Skill */}
        <div className="flex items-center gap-1.5">
          {!isAnyUpdating && onCheckUpdates && skills.length > 0 && (
            checkDone && updatesCount === 0 ? (
              <span className="inline-flex items-center justify-center h-7 px-2.5 text-xs text-success font-medium gap-1.5">
                <Check className="h-3.5 w-3.5" />
                {t('skills.updateDone')}
              </span>
            ) : (
              <Button variant="ghost" size="sm" className="h-7 px-2.5 text-xs font-medium gap-1.5 text-muted-foreground hover:bg-primary/10 hover:text-primary transition-colors cursor-pointer"
                disabled={isCheckingUpdates}
                onClick={() => {
                  void handleCheckUpdates();
                }}>
                <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn("h-3.5 w-3.5 shrink-0", isCheckingUpdates && "animate-spin")}><path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/></svg>
                {t('skills.checkUpdates')}
              </Button>
            )
          )}
          
          {/* 路径不存在时隐藏 Add 按钮 */}
          {pathExists && (
            <Button
              variant="secondary"
              size="sm"
              className="h-7 px-2.5 sm:px-3 text-xs font-semibold gap-1.5 shadow-none text-primary/80 bg-primary/[0.04] hover:bg-primary/10 hover:text-primary border border-transparent cursor-pointer transition-all"
              onClick={onAdd}
            >
              <Plus className="h-3.5 w-3.5 shrink-0" />
              {t('skills.add')}
            </Button>
          )}
        </div>
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
                const updateStatus = updatingSkills.get(
                  getSkillIdentityKey({ name: skill.name, scope: skill.scope, projectPath })
                );
                return (
                  <SkillCard
                    key={skill.name}
                    skill={skill}
                    displayScope={scope}
                    hasConflict={conflictSkillNames.has(skill.name)}
                    updateStatus={updateStatus}
                    projectPath={scope === 'project' ? projectPath : undefined}
                    agentDisplayNames={agentDisplayNames}
                    riskLevel={auditCache[skill.name]?.risk}
                    onClick={onSkillClick}
                    onUpdate={(name) => onUpdate(name, scope)}
                    onDelete={onDelete}
                    onCopyToProject={onCopyToProject}
                    onManageAgents={onManageAgents}
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
