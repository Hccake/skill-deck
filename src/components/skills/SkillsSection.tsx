// src/components/skills/SkillsSection.tsx
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus, Check, ArrowUpCircle, RefreshCw, CircleAlert } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { CrossfadeSwap } from '@/components/ui/crossfade-swap';
import { SkillCard } from './SkillCard';
import { ProjectUnavailableState } from './EmptyStates';
import { getSkillIdentityKey } from '@/lib/skills/identity';
import { cn } from '@/lib/utils';
import type { AgentId, InstalledSkill, SkillAuditData, SkillScope, SourceUpdateCheckInfo, UpdateCheckOutcome } from '@/bindings';
import {
  buildUpdatePlan,
  isSkillUpdateActive,
  resolveSkillMaintenanceAction,
  resolveEvidenceFailureReasonI18nKey,
  resolveUpdateStatusLabelI18nKey,
  hasIncompleteUpdateCheck,
  hasCommittedUpdateComparison,
  providerCooldownDeadline,
  type SkillUpdateDisplayStatus,
  type SkillListItem,
  type UpdatePlan,
} from '@/stores/skills-utils';
import { useMutationStore } from '@/stores/mutation';

// 提升默认值避免重复创建 — rerender-memo-with-default-value 规则
const EMPTY_CONFLICT_SET = new Set<string>();
const EMPTY_DISPLAY_NAMES = new Map<AgentId, string>();
const EMPTY_AUDIT_CACHE: Record<string, SkillAuditData> = {};
const EMPTY_SOURCE_DIAGNOSTICS: SourceUpdateCheckInfo[] = [];

interface SkillsSectionProps {
  title: string;
  skills: SkillListItem[];
  /** 当前 Environment 的完整来源诊断，不受列表筛选影响。 */
  sourceDiagnostics?: SourceUpdateCheckInfo[];
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
  /** Automatic 检查是否正在进行（与 Force busy 分离） */
  isAutomaticCheckingUpdates?: boolean;
  /** 是否已经有至少一个可表达的有效比较结论 */
  hasCommittedComparison?: boolean;
  /** 当前列表是否处于搜索或 Agent 筛选状态 */
  filterActive?: boolean;
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
  onCheckUpdates?: () => Promise<UpdateCheckOutcome | null>;
  emptyState?: React.ReactNode;
}

export const SkillsSection = memo(function SkillsSection({
  title,
  skills,
  sourceDiagnostics = EMPTY_SOURCE_DIAGNOSTICS,
  scope,
  conflictSkillNames = EMPTY_CONFLICT_SET,
  pathExists = true,
  projectPath,
  updatingSkills,
  isCheckingUpdates = false,
  isAutomaticCheckingUpdates = false,
  hasCommittedComparison = false,
  filterActive = false,
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
  const { t, i18n } = useTranslation();
  const writeBlocked = useMutationStore((state) => state.activeMutation !== null);

  // 单次遍历派生所有更新相关状态（js-combine-iterations）— 仅统计当前 section 的 skills
  let isAnyUpdating = false;
  let completedCount = 0;
  let totalUpdating = 0;
  let updateCheckFailureCount = 0;
  let incompleteCheckCount = 0;
  let maintenanceCount = 0;
  let hasVisibleCommittedComparison = false;
  let latestFailureSkill: SkillListItem | null = null;
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
    const hasCommittedSkillComparison = hasCommittedUpdateComparison(skill);
    if (hasCommittedSkillComparison) hasVisibleCommittedComparison = true;
    if (hasIncompleteUpdateCheck(skill)) {
      if (!hasCommittedSkillComparison) {
        incompleteCheckCount++;
      } else if (skill.updateStatus === 'deletedUpstream') {
        maintenanceCount++;
      }
    } else if (updateStatusLabelKey === 'skills.updateStatusLabel.checkFailed') {
      updateCheckFailureCount++;
    } else if (updateStatusLabelKey && updateStatusLabelKey !== 'skills.updateStatusLabel.available') {
      maintenanceCount++;
    }
    if (skill.updateEvidence?.lastAttempt?.failure) {
      const latestCheckedAt = latestFailureSkill?.updateEvidence?.lastAttempt?.checkedAtEpochMs ?? -1;
      if (skill.updateEvidence.lastAttempt.checkedAtEpochMs > latestCheckedAt) {
        latestFailureSkill = skill;
      }
    }
  }
  const updatePlanPreview = useMemo(
    () => buildUpdatePlan(skills, scope, scope === 'project' ? projectPath : undefined),
    [projectPath, scope, skills]
  );
  const updatesCount = updatePlanPreview.updatableCount;
  const incompleteTotal = incompleteCheckCount + updateCheckFailureCount;
  const checkableCount = skills.filter((skill) => skill.canCheckForUpdates === true).length;
  const latestFailure = latestFailureSkill?.updateEvidence?.lastAttempt?.failure ?? null;
  const latestAttemptAt = latestFailureSkill?.updateEvidence?.lastAttempt?.checkedAtEpochMs ?? null;
  const cooldownDeadline = providerCooldownDeadline([
    ...sourceDiagnostics,
    ...skills.flatMap((skill) => skill.updateEvidence ? [skill.updateEvidence] : []),
  ]);
  const [cooldownNow, setCooldownNow] = useState(() => Date.now());
  const cooldownActive = cooldownDeadline != null && cooldownDeadline > cooldownNow;
  useEffect(() => {
    if (cooldownDeadline == null) return undefined;
    const timer = setTimeout(() => setCooldownNow(Date.now()), Math.max(0, cooldownDeadline - Date.now()));
    return () => clearTimeout(timer);
  }, [cooldownDeadline]);

  const showUpToDate = pathExists
    && (!filterActive || skills.length > 0)
    && !isAnyUpdating
    && hasCommittedComparison
    && hasVisibleCommittedComparison
    && updatesCount === 0
    && incompleteTotal === 0
    && maintenanceCount === 0;
  const summaryItems = isAnyUpdating
    ? [t('skills.updateAllProgress', { completed: completedCount, total: totalUpdating })]
    : incompleteTotal > 0 || maintenanceCount > 0
      ? [
          incompleteTotal > 0
            ? t('skills.updateCheckIncompleteCount', { count: incompleteTotal })
            : null,
          maintenanceCount > 0
            ? t('skills.uncheckableUpdateCount', { count: maintenanceCount })
            : null,
        ].filter((item): item is string => item != null)
      : updatesCount > 0
        ? [`${updatesCount} ${t(updatesCount === 1 ? 'skills.update' : 'skills.updates')}`]
        : showUpToDate
          ? [t('skills.upToDate')]
          : [];
  const summaryTransitionKey = summaryItems.join('\u0000') || 'empty';
  const warningTitle = latestFailure
    ? [
        t('skills.updateStatusLabel.checkIncomplete'),
        t(resolveEvidenceFailureReasonI18nKey(latestFailure.reason)),
        latestAttemptAt != null
          ? t('skills.updateEvidence.lastAttempt', {
              time: new Date(latestAttemptAt).toLocaleString(i18n.language),
            })
          : null,
        latestFailure.retryAtEpochMs
          ? t('skills.updateEvidence.retryAt', {
              time: new Date(latestFailure.retryAtEpochMs).toLocaleString(i18n.language),
            })
          : null,
      ].filter((line): line is string => line != null)
    : [];

  // 检测 Force true → false 转换，短暂显示完成态；Automatic spinner 延迟 200ms。
  const [checkDone, setCheckDone] = useState(false);
  const [showAutomaticSpinner, setShowAutomaticSpinner] = useState(false);
  const hideCheckDoneTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (hideCheckDoneTimerRef.current) {
        clearTimeout(hideCheckDoneTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!isAutomaticCheckingUpdates) {
      const timer = setTimeout(() => setShowAutomaticSpinner(false), 0);
      return () => clearTimeout(timer);
    }
    const timer = setTimeout(() => setShowAutomaticSpinner(true), 200);
    return () => clearTimeout(timer);
  }, [isAutomaticCheckingUpdates]);

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
    if (!onCheckUpdates) return;
    const outcome = await onCheckUpdates();
    if (outcome !== 'completed') return;
    showCheckDone();
  }, [onCheckUpdates, showCheckDone]);

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
          {pathExists && (
            <span
              data-testid="update-check-progress-slot"
              className="inline-flex h-5 w-5 shrink-0 items-center justify-center text-muted-foreground"
            >
              {showAutomaticSpinner ? (
                <RefreshCw
                  role="img"
                  aria-label={t('skills.checking')}
                  className="h-3.5 w-3.5 animate-spin motion-reduce:animate-none"
                />
              ) : latestFailure ? (
                <span
                  tabIndex={0}
                  title={warningTitle.join('\n')}
                  className="inline-flex h-5 w-5 items-center justify-center rounded-sm text-warning outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                  aria-label={t('skills.updateStatusLabel.checkIncomplete')}
                >
                  <CircleAlert className="h-3.5 w-3.5" />
                </span>
              ) : null}
            </span>
          )}
          {pathExists ? (
            <span
              data-testid="update-summary-slot"
              className="inline-flex h-10 w-72 max-w-full shrink-0 items-center text-xs"
              aria-live="polite"
              aria-atomic="true"
            >
              <CrossfadeSwap transitionKey={summaryTransitionKey} className="w-full">
                {summaryItems.length > 0 ? (
                  <span className="flex w-full flex-wrap items-center gap-x-2 gap-y-0.5">
                    {summaryItems.map((item) => (
                      <span
                        key={item}
                        className={cn(
                          'inline-flex items-center gap-1.5 font-medium',
                          isAnyUpdating ? 'text-primary' : 'text-muted-foreground/80',
                        )}
                      >
                        <span aria-hidden="true" className="text-border">·</span>
                        {item}
                      </span>
                    ))}
                  </span>
                ) : null}
              </CrossfadeSwap>
            </span>
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
                    {t('skills.checkCompleted')}
                  </span>
                ) : (
                  <Button variant="ghost" size="sm" className="h-7 px-2 text-xs font-medium gap-1.5 text-muted-foreground hover:bg-primary/10 hover:text-primary transition-colors cursor-pointer"
                    disabled={isCheckingUpdates || cooldownActive}
                    title={cooldownActive && cooldownDeadline
                      ? t('skills.updateEvidence.retryAt', {
                          time: new Date(cooldownDeadline).toLocaleString(i18n.language),
                        })
                      : undefined}
                    onClick={() => {
                      void handleCheckUpdates();
                    }}>
                    <RefreshCw className={cn("h-3.5 w-3.5 shrink-0", isCheckingUpdates && "animate-spin")} />
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
