// src/components/skills/SkillCard.tsx
import { useEffect, useRef, memo } from 'react';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { getSkillIdentity, isSameSkillIdentity } from '@/lib/skills/identity';
import { cn, formatTime, toTitleCase } from '@/lib/utils';
import {
  ArrowUpCircle,
  Trash2,
  ExternalLink,
  Globe,
  Folder,
  AlertTriangle,
  FolderOutput,
  Pencil,
  Wrench,
} from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { PopConfirm } from '@/components/ui/pop-confirm';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import type { AgentType, InstalledSkill, RiskLevel, SkillScope, SkillUpdateCheckStatus } from '@/bindings';
import {
  resolveUpdateHintI18nKey,
  resolveUpdateStatusLabelI18nKey,
  resolveSkillMaintenanceAction,
} from '@/stores/skills-utils';
import { RiskBadge } from './RiskBadge';

/** 默认空 Map，避免每次 render 创建新引用 — rerender-memo-with-default-value 规则 */
const EMPTY_DISPLAY_NAMES = new Map<AgentType, string>();

function phaseToPercent(phase: string | null): string {
  switch (phase) {
    case 'cloning': return '35%';
    case 'installing': return '70%';
    case 'writing_lock': return '90%';
    default: return '10%';
  }
}

function phaseToI18nKey(phase: string | null): string {
  switch (phase) {
    case 'cloning': return 'skills.updatePhaseCloning';
    case 'installing': return 'skills.updatePhaseInstalling';
    case 'writing_lock': return 'skills.updatePhaseWritingLock';
    default: return 'skills.updatePhaseCloning';
  }
}

interface SkillCardProps {
  skill: InstalledSkill & {
    updateStatus?: SkillUpdateCheckStatus | null;
    updateReason?: string | null;
  };
  /** 当前显示的 scope（用于决定图标） */
  displayScope: SkillScope;
  /** 是否存在冲突（同时在 project 和 global 安装） */
  hasConflict?: boolean;
  /** 更新状态（来自 updatingSkills Map） */
  updateStatus?: 'queued' | 'updating' | 'done' | 'failed';
  /** 当前 project scope 的项目路径 */
  projectPath?: string;
  /** Agent display name 映射（agentId → displayName） */
  agentDisplayNames?: Map<AgentType, string>;
  /** 安全审计风险等级 */
  riskLevel?: RiskLevel;
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
  projectPath,
  agentDisplayNames = EMPTY_DISPLAY_NAMES,
  riskLevel,
  onClick,
  onUpdate,
  onDelete,
  onCopyToProject,
  onManageAgents,
  onRepairSource,
}: SkillCardProps) {
  const { t, i18n } = useTranslation();
  const skillName = skill.name;
  const skillScope = skill.scope;

  // React1: useRef 替代 useState — rerender-use-ref-transient-values
  // updatePhase 频繁更新（Tauri 事件驱动），使用 ref 避免不必要的 re-render
  const progressBarRef = useRef<HTMLDivElement>(null);
  const phaseBadgeRef = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    if (updateStatus !== 'updating') return;

    const currentIdentity = getSkillIdentity({ name: skillName, scope: skillScope }, projectPath);
    const unlisten = listen<{ skillName: string; scope: SkillScope; projectPath?: string | null; phase: string }>('update-progress', (event) => {
      if (isSameSkillIdentity(currentIdentity, {
        name: event.payload.skillName,
        scope: event.payload.scope,
        projectPath: event.payload.projectPath,
      })) {
        const phase = event.payload.phase;
        // 直接操作 DOM — 不触发 React re-render
        if (progressBarRef.current) {
          progressBarRef.current.style.width = phaseToPercent(phase);
        }
        if (phaseBadgeRef.current) {
          phaseBadgeRef.current.textContent = t(phaseToI18nKey(phase));
        }
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [projectPath, skillName, skillScope, t, updateStatus]);

  const ScopeIcon = displayScope === 'global' ? Globe : Folder;
  const scopeTooltip = t(`skills.scopeIcon.${displayScope}`);
  const conflictTooltip =
    displayScope === 'project'
      ? t('skills.conflict.alsoInGlobal')
      : t('skills.conflict.alsoInProject');
  const canShowUpdateAction = skill.hasUpdate === true && skill.canRunUpdate !== false;
  const maintenanceAction = updateStatus ? 'none' : resolveSkillMaintenanceAction(skill);
  const canShowDirectReinstallAction = maintenanceAction === 'direct-reinstall' && Boolean(onUpdate);
  const canShowRepairAction = maintenanceAction === 'repair-source' && Boolean(onRepairSource);
  const updateStatusLabelKey = skill.hasUpdate ? resolveUpdateStatusLabelI18nKey(skill) : null;
  const updateHintKey = !skill.hasUpdate ? resolveUpdateHintI18nKey(skill.updateReason) : null;

  return (
      <Card
        className={cn(
          "relative py-0 gap-0 cursor-pointer transition-colors border border-border/80 bg-surface rounded-lg hover:border-primary/20 hover:bg-accent/30",
          skill.hasUpdate && "border-l-warning border-l-2"
        )}
        onClick={() => onClick?.(skill)}
      >
        <CardContent className="p-3 sm:p-4">
          {/* Row 1: Scope Icon + Name + Conflict Icon + Actions */}
          <div className="flex items-center justify-between mb-2">
            <div className="flex items-center gap-2 sm:gap-2.5">
              {/* Scope Icon */}
              <Tooltip>
                <TooltipTrigger asChild>
                  <div className="flex h-8 w-8 items-center justify-center rounded-lg border border-black/5 dark:border-white/[0.08] bg-muted/50 shadow-[inset_0_1px_0_rgba(255,255,255,0.5)] dark:shadow-none bg-gradient-to-br from-background to-muted">
                    <ScopeIcon className="h-4 w-4 text-foreground/80 drop-shadow-sm" />
                  </div>
                </TooltipTrigger>
                <TooltipContent>
                  <p>{scopeTooltip}</p>
                </TooltipContent>
              </Tooltip>

              {/* Skill Name */}
              <h3 className="text-sm font-heading font-bold tracking-tight text-foreground">{skill.name}</h3>

              {/* Risk Badge */}
              {riskLevel ? <RiskBadge risk={riskLevel} /> : null}

              {updateStatusLabelKey ? (
                <Badge
                  variant="outline"
                  className={cn("text-xs px-1.5 py-0", skill.hasUpdate ? "text-warning" : "text-muted-foreground")}
                >
                  {t(updateStatusLabelKey)}
                </Badge>
              ) : null}

              {/* Plugin Name Badge */}
              {skill.pluginName ? (
                <Badge variant="secondary" className="text-xs px-1.5 py-0">
                  {toTitleCase(skill.pluginName)}
                </Badge>
              ) : null}

              {/* Conflict Icon */}
              {hasConflict ? (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <AlertTriangle className="h-4 w-4 text-warning" />
                  </TooltipTrigger>
                  <TooltipContent>
                    <p>{conflictTooltip}</p>
                  </TooltipContent>
                </Tooltip>
              ) : null}
            </div>

            {/* Action buttons — React2: 三元条件渲染 (rendering-conditional-render) */}
            <div className="flex items-center gap-0.5 sm:gap-1">
              {updateStatus === 'queued' ? (
                <Badge variant="outline" className="text-xs text-muted-foreground">
                  {t('skills.queued')}
                </Badge>
              ) : null}
              {updateStatus === 'updating' ? (
                <Badge variant="outline" className="text-xs text-warning animate-pulse">
                  <span ref={phaseBadgeRef}>{t('skills.updatePhaseCloning')}</span>
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
                  className="h-7 w-7 text-warning hover:text-warning hover:bg-warning/10 cursor-pointer"
                  aria-label={t('skills.actions.update')}
                  title={t('skills.actions.update')}
                  onClick={(e) => {
                    e.stopPropagation();
                    onUpdate?.(skill.name);
                  }}
                >
                  <ArrowUpCircle className="h-3.5 w-3.5" />
                </Button>
              ) : null}
              {canShowDirectReinstallAction ? (
                <PopConfirm
                  title={t('skills.reinstallConfirm.title')}
                  description={t('skills.reinstallConfirm.description')}
                  confirmLabel={t('skills.reinstallConfirm.confirm')}
                  cancelLabel={t('common.cancel')}
                  onConfirm={() => onUpdate?.(skill.name)}
                >
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7 text-warning hover:text-warning hover:bg-warning/10 cursor-pointer"
                    aria-label={t('skills.actions.reinstall')}
                    title={t('skills.actions.reinstall')}
                    onClick={(e) => {
                      e.stopPropagation();
                    }}
                  >
                    <Wrench className="h-3.5 w-3.5" />
                  </Button>
                </PopConfirm>
              ) : null}
              {canShowRepairAction ? (
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 text-warning hover:text-warning hover:bg-warning/10 cursor-pointer"
                  aria-label={t('skills.actions.repairSource')}
                  title={t('skills.actions.repairSource')}
                  onClick={(e) => {
                    e.stopPropagation();
                    onRepairSource?.(skill);
                  }}
                >
                  <Wrench className="h-3.5 w-3.5" />
                </Button>
              ) : null}
              {displayScope === 'project' && onCopyToProject ? (
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 text-muted-foreground hover:text-primary hover:bg-primary/10 cursor-pointer"
                  aria-label={t('skills.actions.copyToProject')}
                  title={t('skills.actions.copyToProject')}
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
                  aria-label={t('skills.manageAgents.title')}
                  title={t('skills.manageAgents.title')}
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
          <p className="text-sm text-muted-foreground mb-2 leading-relaxed">
            {skill.description}
          </p>

          {/* Row 3: Source + Updated */}
          <div className="flex items-center gap-2 text-xs text-muted-foreground mb-3">
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

          {updateHintKey ? (
            <div className="mb-3 rounded-md border border-border/60 bg-muted/25 px-2.5 py-1.5 text-xs leading-relaxed text-muted-foreground">
              {t(updateHintKey)}
            </div>
          ) : null}

          {/* Row 4: Agents */}
          <div className="flex items-center gap-2 flex-wrap">
            {skill.agents.map((agentId) => (
              <span
                key={agentId}
                className="inline-flex items-center gap-1 rounded-full bg-primary/10 px-3 py-1 text-[11px] font-bold text-primary"
              >
                {agentDisplayNames.get(agentId) ?? agentId}
              </span>
            ))}
          </div>
        </CardContent>
        {/* Bug2 修复：底部极细进度条，无文字标签 */}
        {updateStatus === 'updating' ? (
          <div className="absolute bottom-0 left-0 right-0">
            <div className="h-0.5 bg-warning/20 overflow-hidden ">
              <div ref={progressBarRef} className="h-full bg-warning transition-all duration-500" style={{ width: '10%' }} />
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
