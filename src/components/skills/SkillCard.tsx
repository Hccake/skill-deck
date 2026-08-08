// src/components/skills/SkillCard.tsx
import { memo, useCallback, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { cn, formatTime, toTitleCase } from '@/lib/utils';
import {
  ArrowUpCircle,
  Trash2,
  ExternalLink,
  Globe,
  Folder,
  AlertTriangle,
  CircleAlert,
  Info,
  FolderOutput,
  PackagePlus,
  Pencil,
  Wrench,
} from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { CrossfadeSwap } from '@/components/ui/crossfade-swap';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import type { AgentId, InstalledSkill, RiskLevel, InstalledSkillLocation } from '@/bindings';
import {
  hasIncompleteUpdateCheck,
  hasCommittedUpdateComparison,
  resolveEvidenceFailureNextStepI18nKey,
  resolveEvidenceFailureReasonI18nKey,
  resolveUpdateHintI18nKey,
  resolveUpdateStatusLabelI18nKey,
  isSkillUpdateActive,
  resolveSkillUpdatePhaseI18nKey,
  resolveSkillMaintenanceAction,
  type SkillUpdateDisplayStatus,
  type SkillListItem,
} from '@/stores/skills-utils';
import { RiskBadge } from './RiskBadge';

/** 默认空 Map，避免每次 render 创建新引用 — rerender-memo-with-default-value 规则 */
const EMPTY_DISPLAY_NAMES = new Map<AgentId, string>();

interface SkillCardProps {
  skill: SkillListItem;
  /** 当前显示的 scope（用于决定图标） */
  displayScope: InstalledSkillLocation;
  /** 是否存在冲突（同时在 project 和 global 安装） */
  hasConflict?: boolean;
  /** 更新状态（由 workflow operation 投影） */
  updateStatus?: SkillUpdateDisplayStatus;
  /** 当前 project scope 的项目路径 */
  projectPath?: string;
  /** Agent display name 映射（agentId → displayName） */
  agentDisplayNames?: Map<AgentId, string>;
  /** 安全审计风险等级 */
  riskLevel?: RiskLevel;
  /** 其他写操作进行中，禁止发起新的 Skill 写入 */
  writeBlocked?: boolean;
  /** 点击卡片打开详情 */
  onClick?: (skill: InstalledSkill) => void;
  onUpdate?: (skillName: string) => void;
  onDelete?: (skill: InstalledSkill) => void;
  onCopyToProject?: (skill: InstalledSkill) => void;
  onManageAgents?: (skill: InstalledSkill) => void;
  onRepairSource?: (skill: InstalledSkill) => void;
}

export const SkillCard = memo(function SkillCard({
  skill,
  displayScope,
  hasConflict = false,
  updateStatus,
  agentDisplayNames = EMPTY_DISPLAY_NAMES,
  riskLevel,
  writeBlocked = false,
  onClick,
  onUpdate,
  onDelete,
  onCopyToProject,
  onManageAgents,
  onRepairSource,
}: SkillCardProps) {
  const { t, i18n } = useTranslation();
  const effectiveAgents = Array.from(new Set(skill.associatedAgents));
  const duplicateCopyCount = skill.duplicateCopyCount ?? 0;

  const progressBarRef = useRef<HTMLDivElement>(null);
  const pointerDownRef = useRef<{ x: number; y: number } | null>(null);


  const handlePointerDown = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    pointerDownRef.current = { x: event.clientX, y: event.clientY };
  }, []);

  const handleCardClick = useCallback((event: React.MouseEvent<HTMLDivElement>) => {
    const selectedText = window.getSelection()?.toString();
    if (selectedText) return;

    const pointerDown = pointerDownRef.current;
    pointerDownRef.current = null;
    if (pointerDown) {
      const distance = Math.hypot(event.clientX - pointerDown.x, event.clientY - pointerDown.y);
      if (distance > 4) return;
    }

    onClick?.(skill);
  }, [onClick, skill]);

  const ScopeIcon = displayScope === 'global' ? Globe : Folder;
  const scopeTooltip = t(`skills.scopeIcon.${displayScope}`);
  const conflictTooltip =
    displayScope === 'project'
      ? t('skills.conflict.alsoInGlobal')
      : t('skills.conflict.alsoInProject');
  const isDeletedUpstream = skill.updateStatus === 'deletedUpstream' || skill.updateReason === 'deletedUpstream';
  const canShowUpdateAction = skill.hasUpdate === true && skill.canRunUpdate !== false && !isDeletedUpstream;
  const maintenanceAction = updateStatus ? 'none' : resolveSkillMaintenanceAction(skill);
  const canShowDirectReinstallAction = maintenanceAction === 'direct-reinstall' && Boolean(onUpdate);
  const canShowRepairAction = (maintenanceAction === 'repair-source' || isDeletedUpstream) && Boolean(onRepairSource);
  const repairActionTitle = isDeletedUpstream
    ? t('skills.updatePlan.deletedUpstreamActionRepair')
    : t('skills.actions.repairSource');
  const hasStatusDrivenAction = (canShowUpdateAction && !updateStatus)
    || canShowDirectReinstallAction
    || canShowRepairAction;
  const hasCommittedUpdateConclusion = hasCommittedUpdateComparison(skill);
  const updateStatusLabelKey = resolveUpdateStatusLabelI18nKey(
    hasCommittedUpdateConclusion ? { ...skill, updateAttempt: null } : skill,
  );
  const activeUpdatePhase = isSkillUpdateActive(updateStatus) ? updateStatus : null;
  const typedFailure = skill.updateEvidence?.lastAttempt?.failure ?? null;
  const updateHintKey = !skill.hasUpdate ? resolveUpdateHintI18nKey(skill.updateReason) : null;
  const isIncompleteCheck = hasIncompleteUpdateCheck(skill);
  const isUpdateCheckFailure = (!hasCommittedUpdateConclusion && isIncompleteCheck)
    || updateStatusLabelKey === 'skills.updateStatusLabel.checkFailed';
  const failureReasonKey = typedFailure
    ? resolveEvidenceFailureReasonI18nKey(typedFailure.reason)
    : updateHintKey;
  const failureNextStepKey = typedFailure
    ? resolveEvidenceFailureNextStepI18nKey(typedFailure.reason)
    : null;
  const isAttentionHint = skill.updateReason === 'missing-skill-path'
    || skill.updateReason === 'missingRemoteHash'
    || isDeletedUpstream;
  const updateStatusLabelClassName =
    updateStatusLabelKey === 'skills.updateStatusLabel.available'
      ? 'bg-primary/10 text-primary'
      : updateStatusLabelKey === 'skills.updateStatusLabel.autoCheckUnavailable'
        ? 'bg-muted text-muted-foreground'
        : isUpdateCheckFailure
          ? 'gap-1 text-warning'
          : 'bg-warning/10 text-warning';
  const warningTransitionKey = hasCommittedUpdateConclusion && isIncompleteCheck && failureReasonKey
    ? `${failureReasonKey}:${typedFailure?.retryAtEpochMs ?? ''}`
    : 'none';

  return (
    <Card
      className={cn(
        "group relative py-0 gap-0 cursor-pointer transition-all duration-200 border border-border bg-card rounded-xl hover:shadow-sm hover:border-primary/40"
      )}
      onPointerDown={handlePointerDown}
      onClick={handleCardClick}
    >
      <CardContent className="flex flex-col gap-2 p-4">
        {/* Row 1: Scope Icon + Name + Conflict Icon + Actions */}
        <div data-testid="skill-card-header" className="flex items-center justify-between gap-3">
          <div className="flex min-w-0 items-center gap-2">
            {/* Scope Icon */}
            <Tooltip>
              <TooltipTrigger asChild>
                <div
                  data-testid="skill-scope-marker"
                  className="flex h-6 w-6 shrink-0 items-center justify-center rounded bg-muted/60 text-muted-foreground ring-1 ring-inset ring-border/50"
                >
                  <ScopeIcon className="h-3.5 w-3.5 text-foreground/70" />
                </div>
              </TooltipTrigger>
              <TooltipContent>
                <p>{scopeTooltip}</p>
              </TooltipContent>
            </Tooltip>

            <div className="min-w-0 space-y-1">
              <div className="flex min-w-0 items-center gap-2">
                {/* Skill Name */}
                <h3 className="truncate text-[15px] font-heading font-semibold leading-tight tracking-tight text-foreground">{skill.name}</h3>

                {/* Risk Badge */}
                {riskLevel ? <RiskBadge risk={riskLevel} /> : null}

                <CrossfadeSwap transitionKey={updateStatusLabelKey ?? 'none'}>
                  {updateStatusLabelKey ? (
                    isUpdateCheckFailure && failureReasonKey ? (
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <span
                            tabIndex={0}
                            className={cn(
                              "inline-flex h-[20px] items-center rounded-sm px-1.5 text-[11px] font-medium outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
                              updateStatusLabelClassName
                            )}
                          >
                            <CircleAlert className="h-3 w-3 shrink-0" />
                            {t(updateStatusLabelKey)}
                          </span>
                        </TooltipTrigger>
                        <TooltipContent className="max-w-72 space-y-1 whitespace-normal">
                          <p>{t(failureReasonKey)}</p>
                          {failureNextStepKey ? <p>{t(failureNextStepKey)}</p> : null}
                          {typedFailure?.retryAtEpochMs ? (
                            <p>{t('skills.updateEvidence.retryAt', {
                              time: new Date(typedFailure.retryAtEpochMs).toLocaleString(i18n.language),
                            })}</p>
                          ) : null}
                        </TooltipContent>
                      </Tooltip>
                    ) : (
                      <span className={cn(
                        "inline-flex h-[20px] items-center rounded-sm px-1.5 text-[11px] font-medium",
                        updateStatusLabelClassName
                      )}>
                        {t(updateStatusLabelKey)}
                      </span>
                    )
                  ) : null}
                </CrossfadeSwap>

                <CrossfadeSwap transitionKey={warningTransitionKey}>
                  {hasCommittedUpdateConclusion && isIncompleteCheck && failureReasonKey ? (
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <span
                          data-testid="skill-update-warning"
                          tabIndex={0}
                          className="inline-flex h-5 w-5 items-center justify-center rounded-sm text-warning outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                          aria-label={t('skills.updateStatusLabel.checkIncomplete')}
                        >
                          <CircleAlert className="h-3.5 w-3.5" />
                        </span>
                      </TooltipTrigger>
                      <TooltipContent className="max-w-72 space-y-1 whitespace-normal">
                        <p>{t('skills.updateStatusLabel.checkIncomplete')}</p>
                        <p>{t(failureReasonKey)}</p>
                        {failureNextStepKey ? <p>{t(failureNextStepKey)}</p> : null}
                        {typedFailure?.retryAtEpochMs ? (
                          <p>{t('skills.updateEvidence.retryAt', {
                            time: new Date(typedFailure.retryAtEpochMs).toLocaleString(i18n.language),
                          })}</p>
                        ) : null}
                      </TooltipContent>
                    </Tooltip>
                  ) : null}
                </CrossfadeSwap>

                {/* Conflict Icon */}
                {hasConflict ? (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <AlertTriangle className="h-4 w-4 shrink-0 text-warning" />
                    </TooltipTrigger>
                    <TooltipContent>
                      <p>{conflictTooltip}</p>
                    </TooltipContent>
                  </Tooltip>
                ) : null}
              </div>

              {/* Plugin Name Badge */}
              {skill.pluginName ? (
                <Badge variant="secondary" className="text-xs px-1.5 py-0">
                  {toTitleCase(skill.pluginName)}
                </Badge>
              ) : null}
            </div>
          </div>

          {/* Action buttons — React2: 三元条件渲染 (rendering-conditional-render) */}
          <div className="flex shrink-0 items-center gap-1">
            {activeUpdatePhase ? (
              <Badge variant="outline" className="text-xs text-primary animate-pulse">
                <span>{t(resolveSkillUpdatePhaseI18nKey(activeUpdatePhase))}</span>
              </Badge>
            ) : null}
            {updateStatus === 'done' ? (
              <Badge variant="outline" className="text-xs text-success">
                {t('skills.updateDone')}
              </Badge>
            ) : null}
            {updateStatus === 'failed' ? (
              <Badge variant="outline" className="text-xs text-destructive">
                {t('skills.updateFailed')}
              </Badge>
            ) : null}
            {canShowUpdateAction && !updateStatus ? (
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7 text-primary hover:text-primary hover:bg-primary/10 cursor-pointer"
                aria-label={t('skills.actions.update')}
                title={t('skills.actions.update')}
                disabled={writeBlocked}
                onClick={(e) => {
                  e.stopPropagation();
                  onUpdate?.(skill.name);
                }}
              >
                <ArrowUpCircle className="h-4 w-4" />
              </Button>
            ) : null}
            {canShowDirectReinstallAction ? (
              <AlertDialog>
                <AlertDialogTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7 text-muted-foreground hover:text-primary hover:bg-primary/10 cursor-pointer"
                    aria-label={t('skills.actions.reinstall')}
                    title={t('skills.actions.reinstall')}
                    disabled={writeBlocked}
                    onClick={(event) => event.stopPropagation()}
                  >
                    <Wrench className="h-3.5 w-3.5" />
                  </Button>
                </AlertDialogTrigger>
                <AlertDialogContent onClick={(event) => event.stopPropagation()}>
                  <AlertDialogHeader>
                    <AlertDialogTitle>{t('skills.reinstallConfirm.title')}</AlertDialogTitle>
                    <AlertDialogDescription>{t('skills.reinstallConfirm.description')}</AlertDialogDescription>
                  </AlertDialogHeader>
                  <AlertDialogFooter>
                    <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
                    <AlertDialogAction onClick={() => onUpdate?.(skill.name)}>
                      {t('skills.reinstallConfirm.confirm')}
                    </AlertDialogAction>
                  </AlertDialogFooter>
                </AlertDialogContent>
              </AlertDialog>
            ) : null}
            {canShowRepairAction ? (
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7 text-muted-foreground hover:text-primary hover:bg-primary/10 cursor-pointer"
                aria-label={repairActionTitle}
                title={repairActionTitle}
                disabled={writeBlocked}
                onClick={(e) => {
                  e.stopPropagation();
                  onRepairSource?.(skill);
                }}
              >
                <PackagePlus className="h-3.5 w-3.5" />
              </Button>
            ) : null}
            {hasStatusDrivenAction ? (
              <span className="mx-0.5 h-4 w-px bg-border/70" aria-hidden="true" />
            ) : null}
            {displayScope === 'project' && onCopyToProject ? (
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7 text-muted-foreground hover:text-primary hover:bg-primary/10 cursor-pointer"
                aria-label={t('skills.actions.copyToProject')}
                title={t('skills.actions.copyToProject')}
                disabled={writeBlocked}
                onClick={(e) => {
                  e.stopPropagation();
                  onCopyToProject(skill);
                }}
              >
                <FolderOutput className="h-3.5 w-3.5" />
              </Button>
            ) : null}
            {onManageAgents ? (
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7 text-muted-foreground hover:text-foreground hover:bg-muted/50 cursor-pointer"
                aria-label={t('skills.manageAgents.action')}
                title={t('skills.manageAgents.action')}
                disabled={writeBlocked}
                onClick={(e) => {
                  e.stopPropagation();
                  onManageAgents(skill);
                }}
              >
                <Pencil className="h-3.5 w-3.5" />
              </Button>
            ) : null}
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7 text-muted-foreground hover:text-destructive hover:bg-destructive/10 cursor-pointer"
              aria-label={t('skills.actions.delete')}
              title={t('skills.actions.delete')}
              disabled={writeBlocked}
              onClick={(e) => {
                e.stopPropagation();
                onDelete?.(skill);
              }}
            >
              <Trash2 className="h-3.5 w-3.5" />
            </Button>
          </div>
        </div>

        {/* Row 2: Description */}
        <p className="text-sm leading-[21px] text-muted-foreground line-clamp-2">
          {skill.description}
        </p>

        {/* Row 3: Source + Updated */}
        <div
          data-testid="skill-card-metadata"
          className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground"
        >
          {skill.sourceUrl && skill.source ? (
            <>
              <a
                href={skill.sourceUrl}
                target="_blank"
                rel="noopener noreferrer"
                className="flex items-center gap-1 text-primary hover:text-primary/80 transition-colors cursor-pointer font-medium"
                onClick={(e) => e.stopPropagation()}
              >
                <span className="truncate max-w-[120px] sm:max-w-none">{skill.source}</span>
                <ExternalLink className="h-3 w-3 flex-shrink-0" />
              </a>
              <span className="text-border">·</span>
            </>
          ) : null}
          {skill.gitRef ? (
            <Badge variant="outline" className="text-xs px-1.5 py-0">
              {t('skills.refBadge', { ref: skill.gitRef })}
            </Badge>
          ) : null}
          {skill.gitRef && skill.updatedAt ? (
            <span className="text-border">·</span>
          ) : null}
          {skill.updatedAt ? (
            <span>{t('skills.updated', { time: formatTime(skill.updatedAt, i18n.language) })}</span>
          ) : null}
        </div>

        {updateHintKey && !isUpdateCheckFailure ? (
          <div
            className={cn(
              "flex items-center gap-1 text-xs leading-4",
              isAttentionHint ? "text-warning" : "text-muted-foreground/90"
            )}
          >
            <Info className={cn(
              "-translate-y-px h-3.5 w-3.5 shrink-0",
              isAttentionHint ? "text-warning" : "text-muted-foreground"
            )} />
            <span className={cn(isAttentionHint ? "text-warning" : "text-muted-foreground")}>{t(updateHintKey)}</span>
          </div>
        ) : null}

        {/* Row 4: Agents */}
        <div className="flex items-center gap-1.5 flex-wrap pt-0.5 mt-auto">
          {effectiveAgents.map((agentId) => (
            <span
              key={agentId}
              className="inline-flex h-6 items-center rounded-full bg-primary/10 px-2.5 text-xs font-medium text-primary ring-1 ring-inset ring-primary/20"
            >
              {agentDisplayNames.get(agentId) ?? agentId}
            </span>
          ))}
          {duplicateCopyCount > 0 ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <span
                  className="inline-flex h-6 w-6 items-center justify-center rounded-full text-amber-700 hover:bg-amber-500/10 dark:text-amber-300"
                  aria-label={t('skills.card.extraCopies')}
                >
                  <AlertTriangle className="h-3.5 w-3.5" />
                </span>
              </TooltipTrigger>
              <TooltipContent className="max-w-72 whitespace-normal text-left leading-5">
                <p data-testid="skill-card-extra-copies-tooltip">{t('skills.card.extraCopiesHint')}</p>
              </TooltipContent>
            </Tooltip>
          ) : null}
        </div>
      </CardContent>
      {/* Bug2 修复：底部极细进度条，无文字标签 */}
      {activeUpdatePhase ? (
        <div className="absolute bottom-0 left-0 right-0">
          <div className="h-0.5 bg-primary/15 overflow-hidden ">
            <div ref={progressBarRef} className="h-full bg-primary transition-all duration-500" style={{ width: '10%' }} />
          </div>
        </div>
      ) : null}
      {updateStatus === 'done' ? (
        <div className="absolute bottom-0 left-0 right-0 h-0.5 bg-success transition-opacity duration-700 " />
      ) : null}
      {updateStatus === 'failed' ? (
        <div className="absolute bottom-0 left-0 right-0 h-0.5 bg-destructive " />
      ) : null}
    </Card>
  );
});
