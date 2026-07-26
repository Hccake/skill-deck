// src/components/skills/SkillsSection.tsx
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus, Check, ArrowUpCircle } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { SkillCard } from './SkillCard';
import { ProjectUnavailableState } from './EmptyStates';
import { getSkillIdentityKey } from '@/lib/skills/identity';
import { cn } from '@/lib/utils';
import type { AgentId, InstalledSkill, SkillAuditData, SkillScope } from '@/bindings';
import {
  buildUpdatePlan,
  isSkillUpdateActive,
  resolveSkillMaintenanceAction,
  resolveUpdateStatusLabelI18nKey,
  type SkillUpdateDisplayStatus,
  type SkillListItem,
  type UpdatePlan,
} from '@/stores/skills-utils';
import { useMutationStore } from '@/stores/mutation';

// 提升默认值避免重复创建 — rerender-memo-with-default-value 规则
const EMPTY_CONFLICT_SET = new Set<string>();
const EMPTY_DISPLAY_NAMES = new Map<AgentId, string>();
const EMPTY_AUDIT_CACHE: Record<string, SkillAuditData> = {};

interface SkillsSectionProps {
  title: string;
  skills: SkillListItem[];
  scope: SkillScope;
  conflictSkillNames?: Set<string>;
  /** 项目目录是否存在（仅 project scope） */
  pathExists?: boolean;
  /** 项目路径（仅 project scope，用于提示信息） */
  projectPath?: string;
  /** 各 skill 的更新状态 */
  updatingSkills: Map<string, SkillUpdateDisplayStatus>;
  /** 是否正在检查更新 */
  isCheckingUpdates?: boolean;
  /** Agent display name 映射（agentId → displayName） */
  agentDisplayNames?: Map<AgentId, string>;
  /** 审计数据缓存（skillName → SkillAuditData） */
  auditCache?: Record<string, SkillAuditData>;
  onSkillClick: (skill: InstalledSkill) => void;
  onPrepareUpdate: (skillNames: string[], batch: boolean) => Promise<boolean>;
  onDelete: (skill: InstalledSkill) => void;
  onCopyToProject?: (skill: InstalledSkill) => void;
  onManageAgents?: (skill: InstalledSkill) => void;
  onRepairSource?: (skill: InstalledSkill) => void;
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
  onPrepareUpdate,
  onDelete,
  onCopyToProject,
  onManageAgents,
  onRepairSource,
  onAdd,
  onCheckUpdates,
  emptyState,
}: SkillsSectionProps) {
  const { t } = useTranslation();
  const writeBlocked = useMutationStore((state) => state.activeMutation !== null);

  // 单次遍历派生所有更新相关状态（js-combine-iterations）— 仅统计当前 section 的 skills
  let isAnyUpdating = false;
  let completedCount = 0;
  let totalUpdating = 0;
  let updateCheckFailureCount = 0;
  let maintenanceCount = 0;
  for (const skill of skills) {
    const updatingStatus = updatingSkills.get(
      getSkillIdentityKey({ name: skill.name, scope: skill.scope, projectPath })
    );
    if (updatingStatus) {
      totalUpdating++;
      if (isSkillUpdateActive(updatingStatus)) {
        isAnyUpdating = true;
      } else {
        completedCount++;
      }
    }
    const updateStatusLabelKey = resolveUpdateStatusLabelI18nKey(skill);
    if (updateStatusLabelKey === 'skills.updateStatusLabel.checkFailed') {
      updateCheckFailureCount++;
    } else if (updateStatusLabelKey && updateStatusLabelKey !== 'skills.updateStatusLabel.available') {
      maintenanceCount++;
    }
  }
  const updatePlanPreview = useMemo(
    () => buildUpdatePlan(skills, scope, scope === 'project' ? projectPath : undefined),
    [projectPath, scope, skills]
  );
  const updatesCount = updatePlanPreview.updatableCount;
  const checkableCount = skills.filter((skill) => skill.canCheckForUpdates === true).length;
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

  const openPreparedUpdatePlan = useCallback(async (nextPlan: UpdatePlan, batch: boolean) => {
    if (nextPlan.updatableCount === 0) return;
    const skillNames = nextPlan.groups.flatMap((group) => group.skillNames);
    await onPrepareUpdate(skillNames, batch);
  }, [onPrepareUpdate]);

  const handleOpenUpdatePlan = useCallback(() => {
    void openPreparedUpdatePlan(updatePlanPreview, true);
  }, [openPreparedUpdatePlan, updatePlanPreview]);

  const handleUpdateSkill = useCallback(async (skillName: string) => {
    const skill = skills.find((candidate) => candidate.name === skillName);
    if (skill && resolveSkillMaintenanceAction(skill) === 'direct-reinstall') {
      await onPrepareUpdate([skillName], false);
      return;
    }
    const nextPlan = buildUpdatePlan(
      skills.filter((skill) => skill.name === skillName),
      scope,
      scope === 'project' ? projectPath : undefined,
    );
    await openPreparedUpdatePlan(nextPlan, false);
  }, [onPrepareUpdate, openPreparedUpdatePlan, projectPath, scope, skills]);

  return (
    <>
    <section className="mb-6">
      {/* Section Header */}
      <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2 mb-3">
        <div className="flex items-center gap-1.5 flex-wrap">
          <h2 className="text-sm font-bold tracking-tight text-foreground/90 flex items-center gap-1">
            {title}
            <span className="text-xs font-semibold opacity-50">({skills.length})</span>
          </h2>
          {pathExists && isCheckingUpdates && updatesCount === 0 && (
            <div className="flex items-center gap-1 text-xs">
              <span className="text-border mr-0.5">·</span>
              <span className="text-xs text-muted-foreground">{t('skills.checking')}</span>
            </div>
          )}

          {pathExists && (isAnyUpdating ? (
            <div className="flex items-center gap-1.5 text-xs">
              <span className="text-border mr-0.5">·</span>
              <span className="font-medium text-primary">
                {t('skills.updateAllProgress', { completed: completedCount, total: totalUpdating })}
              </span>
            </div>
          ) : updatesCount > 0 ? (
            <div className="flex items-center gap-1.5 text-xs">
              <span className="text-border mr-0.5">·</span>
              <span className="font-medium text-muted-foreground">
                {`${updatesCount} ${t(updatesCount === 1 ? 'skills.update' : 'skills.updates')}`}
              </span>
            </div>
          ) : null)}
          {pathExists && !isAnyUpdating && updateCheckFailureCount > 0 ? (
            <div className="flex items-center gap-1.5 text-xs">
              <span className="mr-0.5 text-border">·</span>
              <span role="status" className="font-medium text-muted-foreground/80">
                {t('skills.updateCheckFailureCount', { count: updateCheckFailureCount })}
              </span>
            </div>
          ) : null}
          {pathExists && !isAnyUpdating && maintenanceCount > 0 ? (
            <div className="flex items-center gap-1.5 text-xs">
              <span className="text-border mr-0.5">·</span>
              <span className="font-medium text-muted-foreground/80">
                {t('skills.uncheckableUpdateCount', { count: maintenanceCount })}
              </span>
            </div>
          ) : null}
          {pathExists && !isAnyUpdating && updatesCount === 0 && updateCheckFailureCount === 0 && maintenanceCount === 0 && !isCheckingUpdates ? (
            <div className="flex items-center gap-1 text-xs">
              <span className="text-border mr-0.5">·</span>
              <span className="font-medium text-muted-foreground/80">
                {t('skills.upToDate')}
              </span>
            </div>
          ) : null}
        </div>
        
        {/* Right Actions: Secondary maintenance actions + primary add action */}
        <div data-testid="skills-section-actions" className="flex items-center gap-2">
          {pathExists && !isAnyUpdating && (updatesCount > 0 || (onCheckUpdates && skills.length > 0 && checkableCount > 0)) && (
            <div data-testid="skills-section-secondary-actions" className="flex items-center gap-0.5">
              {updatesCount > 0 && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-7 px-2 text-xs font-medium gap-1.5 text-muted-foreground hover:bg-primary/10 hover:text-primary transition-colors cursor-pointer"
                  onClick={handleOpenUpdatePlan}
                  disabled={writeBlocked}
                >
                  <ArrowUpCircle className="h-3.5 w-3.5 shrink-0" />
                  {t('skills.updateAll')}
                </Button>
              )}
              {onCheckUpdates && skills.length > 0 && checkableCount > 0 && (
                checkDone && updatesCount === 0 ? (
                  <span className="inline-flex items-center justify-center h-7 px-2 text-xs text-success font-medium gap-1.5">
                    <Check className="h-3.5 w-3.5" />
                    {t('skills.updateDone')}
                  </span>
                ) : (
                  <Button variant="ghost" size="sm" className="h-7 px-2 text-xs font-medium gap-1.5 text-muted-foreground hover:bg-primary/10 hover:text-primary transition-colors cursor-pointer"
                    disabled={isCheckingUpdates}
                    onClick={() => {
                      void handleCheckUpdates();
                    }}>
                    <svg aria-hidden="true" xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn("h-3.5 w-3.5 shrink-0", isCheckingUpdates && "animate-spin")}><path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/></svg>
                    {t('skills.checkUpdates')}
                  </Button>
                )
              )}
            </div>
          )}
          
          {/* 路径不存在时隐藏 Add 按钮 */}
          {pathExists && (
            <Button
              variant="secondary"
              size="sm"
              className="h-7 px-2.5 sm:px-3 text-xs font-semibold gap-1.5 shadow-none text-primary/80 bg-primary/[0.04] hover:bg-primary/10 hover:text-primary border border-transparent cursor-pointer transition-all"
              onClick={onAdd}
              disabled={writeBlocked}
            >
              <Plus className="h-3.5 w-3.5 shrink-0" />
              {t('skills.add')}
            </Button>
          )}
        </div>
      </div>

      {!pathExists && <ProjectUnavailableState />}

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
                    writeBlocked={writeBlocked}
                    onClick={onSkillClick}
                    onUpdate={handleUpdateSkill}
                    onDelete={onDelete}
                    onCopyToProject={onCopyToProject}
                    onManageAgents={onManageAgents}
                    onRepairSource={onRepairSource}
                  />
                );
              })}
            </div>
          )}
        </>
      )}
    </section>
    </>
  );
});
