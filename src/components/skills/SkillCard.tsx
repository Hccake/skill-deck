import { memo, useCallback, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { openUrl } from '@tauri-apps/plugin-opener';
import { toast } from 'sonner';
import {
  ArrowUpCircle,
  CircleAlert,
  ExternalLink,
  Folder,
  FolderOutput,
  Globe,
  Pencil,
  Trash2,
  Wrench,
} from 'lucide-react';
import { cn, toTitleCase } from '@/lib/utils';
import { formatSkillCardDate } from '@/lib/skill-card-presentation';
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
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type { AgentId, InstalledSkill, InstalledSkillLocation } from '@/bindings';
import {
  hasCommittedUpdateComparison,
  hasIncompleteUpdateCheck,
  isSkillUpdateActive,
  resolveEvidenceFailureNextStepI18nKey,
  resolveEvidenceFailureReasonI18nKey,
  resolveSkillMaintenanceAction,
  resolveSkillUpdatePhaseI18nKey,
  resolveUpdateHintI18nKey,
  resolveUpdateStatusLabelI18nKey,
  type SkillListItem,
  type SkillUpdateDisplayStatus,
} from '@/stores/skills-utils';

const EMPTY_DISPLAY_NAMES = new Map<AgentId, string>();

type AttentionKey =
  | 'skills.card.sourceIncomplete'
  | 'skills.card.sourceMissingUpstream'
  | 'skills.card.updateCheckIncomplete'
  | 'skills.card.duplicateLocations'
  | 'skills.card.duplicateAgentInstall';

function isMissingSource(skill: SkillListItem): boolean {
  return skill.updateReason === 'missing-skill-path' || skill.updateReason === 'missingSource';
}

function isDeletedUpstream(skill: SkillListItem): boolean {
  return skill.updateStatus === 'deletedUpstream'
    || skill.updateReason === 'deletedUpstream'
    || skill.updateReason === 'deleted-upstream';
}

function hasUpdateCheckProblem(skill: SkillListItem): boolean {
  if (isMissingSource(skill) || isDeletedUpstream(skill)) return false;
  if (hasIncompleteUpdateCheck(skill)) return true;
  if (!skill.updateReason) return false;
  return ![
    'missingRemoteHash',
    'missing-remote-hash',
    'unsupportedSource',
    'unsupported-source-type',
    'local-source',
  ].includes(skill.updateReason);
}

function resolveAttentionKeys(skill: SkillListItem, hasDuplicateLocations: boolean): AttentionKey[] {
  const keys: AttentionKey[] = [];
  if (isMissingSource(skill)) keys.push('skills.card.sourceIncomplete');
  else if (isDeletedUpstream(skill)) keys.push('skills.card.sourceMissingUpstream');
  if (hasUpdateCheckProblem(skill)) keys.push('skills.card.updateCheckIncomplete');
  if (hasDuplicateLocations) keys.push('skills.card.duplicateLocations');
  if ((skill.duplicateCopyCount ?? 0) > 0) keys.push('skills.card.duplicateAgentInstall');
  return keys;
}

function isOpenableUrl(value: string | null | undefined): value is string {
  if (!value) return false;
  try {
    const url = new URL(value);
    return url.protocol === 'https:' || url.protocol === 'http:';
  } catch {
    return false;
  }
}

interface SkillCardProps {
  skill: SkillListItem;
  displayScope: InstalledSkillLocation;
  /** 同名 Skill 同时安装在全局和当前项目。 */
  hasDuplicateLocation?: boolean;
  updateStatus?: SkillUpdateDisplayStatus;
  projectPath?: string;
  agentDisplayNames?: Map<AgentId, string>;
  writeBlocked?: boolean;
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
  hasDuplicateLocation = false,
  updateStatus,
  agentDisplayNames = EMPTY_DISPLAY_NAMES,
  writeBlocked = false,
  onClick,
  onUpdate,
  onDelete,
  onCopyToProject,
  onManageAgents,
  onRepairSource,
}: SkillCardProps) {
  const { t, i18n } = useTranslation();
  const progressBarRef = useRef<HTMLDivElement>(null);
  const pointerDownRef = useRef<{ x: number; y: number } | null>(null);
  const effectiveAgents = Array.from(new Set(skill.associatedAgents));
  const scopeIcon = displayScope === 'global' ? Globe : Folder;
  const ScopeIcon = scopeIcon;
  const deletedUpstream = isDeletedUpstream(skill);
  const maintenanceAction = updateStatus ? 'none' : resolveSkillMaintenanceAction(skill);
  const canShowUpdateAction = skill.hasUpdate === true
    && skill.canRunUpdate !== false
    && !deletedUpstream
    && !updateStatus
    && Boolean(onUpdate);
  const canShowDirectReinstallAction = maintenanceAction === 'direct-reinstall' && Boolean(onUpdate);
  const canShowRepairAction = (maintenanceAction === 'repair-source' || deletedUpstream)
    && Boolean(onRepairSource);
  const repairActionTitle = deletedUpstream
    ? t('skills.updatePlan.deletedUpstreamActionRepair')
    : t('skills.actions.repairSource');
  const activeUpdatePhase = isSkillUpdateActive(updateStatus) ? updateStatus : null;
  const hasCommittedUpdateConclusion = hasCommittedUpdateComparison(skill);
  const rawStatusLabelKey = resolveUpdateStatusLabelI18nKey(
    hasCommittedUpdateConclusion ? { ...skill, updateAttempt: null } : skill,
  );
  const titleStatusLabelKey = rawStatusLabelKey && [
    'skills.updateStatusLabel.available',
    'skills.updateStatusLabel.reinstallRequired',
    'skills.updateStatusLabel.autoCheckUnavailable',
  ].includes(rawStatusLabelKey)
    ? rawStatusLabelKey
    : null;
  const statusTransitionKey = activeUpdatePhase
    ? resolveSkillUpdatePhaseI18nKey(activeUpdatePhase)
    : updateStatus === 'done'
      ? 'skills.updateDone'
      : updateStatus === 'failed'
        ? 'skills.updateFailed'
        : titleStatusLabelKey ?? 'none';
  const attentionKeys = resolveAttentionKeys(skill, hasDuplicateLocation);
  const attentionLabels = attentionKeys.map((key) => t(key));
  const typedFailure = skill.updateEvidence?.lastAttempt?.failure ?? null;
  const legacyFailureKey = resolveUpdateHintI18nKey(skill.updateReason);
  const failureReasonKey = typedFailure
    ? resolveEvidenceFailureReasonI18nKey(typedFailure.reason)
    : legacyFailureKey;
  const failureNextStepKey = typedFailure
    ? resolveEvidenceFailureNextStepI18nKey(typedFailure.reason)
    : null;
  const retryAtLabel = typedFailure?.retryAtEpochMs
    ? t('skills.updateEvidence.retryAt', {
        time: new Date(typedFailure.retryAtEpochMs).toLocaleString(i18n.language),
      })
    : null;
  const attentionTitle = hasUpdateCheckProblem(skill) && failureReasonKey
    ? [t(failureReasonKey), failureNextStepKey ? t(failureNextStepKey) : null, retryAtLabel]
      .filter((value): value is string => Boolean(value))
      .join('\n')
    : undefined;
  const sourceLabel = skill.source?.trim() || skill.sourceUrl?.trim() || null;
  const sourceUrl = isOpenableUrl(skill.sourceUrl) ? skill.sourceUrl : null;
  const updatedAt = skill.updatedAt
    ? formatSkillCardDate(skill.updatedAt, i18n.language)
    : null;
  const handlePointerDown = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    pointerDownRef.current = { x: event.clientX, y: event.clientY };
  }, []);

  const handleCardClick = useCallback((event: React.MouseEvent<HTMLDivElement>) => {
    if (window.getSelection()?.toString()) return;
    const pointerDown = pointerDownRef.current;
    pointerDownRef.current = null;
    if (pointerDown) {
      const distance = Math.hypot(event.clientX - pointerDown.x, event.clientY - pointerDown.y);
      if (distance > 4) return;
    }
    onClick?.(skill);
  }, [onClick, skill]);

  const handleOpenSource = useCallback((event: React.MouseEvent<HTMLButtonElement>) => {
    event.stopPropagation();
    if (!sourceUrl) return;
    void openUrl(sourceUrl).catch((error: unknown) => {
      console.error('Failed to open Skill source:', error);
      toast.error(t('skills.card.sourceOpenFailed'));
    });
  }, [sourceUrl, t]);

  const handleTitleClick = useCallback((event: React.MouseEvent<HTMLButtonElement>) => {
    event.stopPropagation();
    onClick?.(skill);
  }, [onClick, skill]);

  const attentionRow = attentionLabels.length > 0 ? (
    <div
      data-testid="skill-card-attention"
      role="note"
      aria-label={attentionLabels.join('，')}
      tabIndex={attentionTitle ? 0 : undefined}
      className="flex items-start gap-1.5 rounded-sm text-xs leading-5 text-warning/90 outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
    >
      <CircleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
      <div className="flex min-w-0 flex-wrap gap-x-1.5">
        {attentionLabels.map((label, index) => (
          <span key={attentionKeys[index]} className="inline-flex max-w-full whitespace-normal break-words">
            {index > 0 ? <span className="mr-1.5 text-warning/50" aria-hidden="true">·</span> : null}
            {label}
          </span>
        ))}
      </div>
    </div>
  ) : null;

  return (
    <Card
      className="group relative gap-0 rounded-xl border border-border bg-card py-0 transition-all duration-200 hover:border-primary/40 hover:shadow-sm"
      onPointerDown={handlePointerDown}
      onClick={handleCardClick}
    >
      <CardContent className="grid grid-cols-[1.5rem_minmax(0,1fr)_auto] items-start gap-x-2.5 p-4">
        <Tooltip>
          <TooltipTrigger asChild>
            <div
              data-testid="skill-scope-marker"
              className="flex size-6 items-center justify-center rounded bg-muted/60 text-muted-foreground ring-1 ring-inset ring-border/50"
            >
              <ScopeIcon className="size-3.5 text-foreground/70" aria-hidden="true" />
            </div>
          </TooltipTrigger>
          <TooltipContent><p>{t(`skills.scopeIcon.${displayScope}`)}</p></TooltipContent>
        </Tooltip>

        <div className="min-w-0 space-y-2">
          <div data-testid="skill-card-title" className="flex min-w-0 items-center gap-2 overflow-hidden">
            {onClick ? (
              <button
                type="button"
                title={skill.name}
                className="min-w-16 shrink cursor-pointer text-left outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                onClick={handleTitleClick}
              >
                <h3 className="truncate font-heading text-[15px] font-semibold leading-5 text-foreground">
                  {skill.name}
                </h3>
              </button>
            ) : (
              <h3 className="truncate font-heading text-[15px] font-semibold leading-5 text-foreground">
                {skill.name}
              </h3>
            )}
            {skill.pluginName ? (
              <span
                title={toTitleCase(skill.pluginName)}
                className="max-w-40 min-w-0 shrink-[2] truncate text-xs text-muted-foreground"
              >
                {toTitleCase(skill.pluginName)}
              </span>
            ) : null}
            <CrossfadeSwap transitionKey={statusTransitionKey} className="shrink-0">
              {activeUpdatePhase ? (
                <Badge variant="outline" className="shrink-0 text-xs text-primary motion-safe:animate-pulse">
                  {t(resolveSkillUpdatePhaseI18nKey(activeUpdatePhase))}
                </Badge>
              ) : updateStatus === 'done' ? (
                <Badge variant="outline" className="shrink-0 text-xs text-success">{t('skills.updateDone')}</Badge>
              ) : updateStatus === 'failed' ? (
                <Badge variant="outline" className="shrink-0 text-xs text-destructive">{t('skills.updateFailed')}</Badge>
              ) : titleStatusLabelKey ? (
                <span className={cn(
                  'inline-flex h-5 shrink-0 items-center rounded-sm px-1.5 text-[11px] font-medium',
                  titleStatusLabelKey === 'skills.updateStatusLabel.available'
                    ? 'bg-primary/10 text-primary'
                    : titleStatusLabelKey === 'skills.updateStatusLabel.autoCheckUnavailable'
                      ? 'bg-muted text-muted-foreground'
                      : 'bg-warning/10 text-warning',
                )}>
                  {t(titleStatusLabelKey)}
                </span>
              ) : null}
            </CrossfadeSwap>
          </div>

          {skill.description ? (
            <p className="line-clamp-2 text-sm leading-[21px] text-muted-foreground">
              {skill.description}
            </p>
          ) : null}

          {sourceLabel || skill.gitRef || updatedAt ? (
            <div
              data-testid="skill-card-metadata"
              className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground [&>span:not(:last-child)]:after:ml-2 [&>span:not(:last-child)]:after:text-border [&>span:not(:last-child)]:after:content-['·']"
            >
              {sourceLabel ? (
                <span className="inline-flex min-w-0 items-center">
                  {sourceUrl ? (
                    <button
                      type="button"
                      aria-label={sourceLabel}
                      title={t('skills.externalLink')}
                      className="inline-flex min-w-0 cursor-pointer items-center gap-1 font-medium text-primary outline-none transition-colors hover:text-primary/80 focus-visible:ring-2 focus-visible:ring-ring/50"
                      onClick={handleOpenSource}
                    >
                      <span className="max-w-48 truncate">{sourceLabel}</span>
                      <ExternalLink className="size-3 shrink-0" aria-hidden="true" />
                    </button>
                  ) : (
                    <span className="max-w-48 truncate">{sourceLabel}</span>
                  )}
                </span>
              ) : null}
              {skill.gitRef ? (
                <span className="inline-flex items-center">
                  <Badge variant="outline" className="px-1.5 py-0 text-xs">
                    {t('skills.refBadge', { ref: skill.gitRef })}
                  </Badge>
                </span>
              ) : null}
              {updatedAt ? (
                <span className="inline-flex items-center">
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <span tabIndex={0} className="rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/50">
                        {t('skills.updated', { time: updatedAt.short })}
                      </span>
                    </TooltipTrigger>
                    <TooltipContent><p>{updatedAt.full}</p></TooltipContent>
                  </Tooltip>
                </span>
              ) : null}
            </div>
          ) : null}

          {attentionTitle && attentionRow ? (
            <Tooltip>
              <TooltipTrigger asChild>{attentionRow}</TooltipTrigger>
              <TooltipContent className="max-w-72 whitespace-pre-line">
                {attentionTitle}
              </TooltipContent>
            </Tooltip>
          ) : attentionRow}

          {effectiveAgents.length > 0 ? (
            <div className="flex flex-wrap items-center gap-1.5 pt-0.5">
              {effectiveAgents.map((agentId) => (
                <span
                  key={agentId}
                  className="inline-flex h-6 items-center rounded-full bg-primary/10 px-2.5 text-xs font-medium text-primary ring-1 ring-inset ring-primary/20"
                >
                  {agentDisplayNames.get(agentId) ?? agentId}
                </span>
              ))}
            </div>
          ) : null}
        </div>

        <div className="flex shrink-0 items-center gap-0.5 pl-1">
          {canShowUpdateAction ? (
            <Button
              variant="ghost"
              size="icon"
              className="size-7 cursor-pointer text-primary hover:bg-primary/10 hover:text-primary"
              aria-label={t('skills.actions.update')}
              title={t('skills.actions.update')}
              disabled={writeBlocked}
              onClick={(event) => {
                event.stopPropagation();
                onUpdate?.(skill.name);
              }}
            >
              <ArrowUpCircle className="size-4" aria-hidden="true" />
            </Button>
          ) : null}
          {canShowDirectReinstallAction ? (
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-7 cursor-pointer text-muted-foreground hover:bg-primary/10 hover:text-primary"
                  aria-label={t('skills.actions.reinstall')}
                  title={t('skills.actions.reinstall')}
                  disabled={writeBlocked}
                  onClick={(event) => event.stopPropagation()}
                >
                  <Wrench className="size-3.5" aria-hidden="true" />
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
              className="size-7 cursor-pointer text-primary hover:bg-primary/10 hover:text-primary"
              aria-label={repairActionTitle}
              title={repairActionTitle}
              disabled={writeBlocked}
              onClick={(event) => {
                event.stopPropagation();
                onRepairSource?.(skill);
              }}
            >
              <Wrench className="size-3.5" aria-hidden="true" />
            </Button>
          ) : null}
          {displayScope === 'project' && onCopyToProject ? (
            <Button
              variant="ghost"
              size="icon"
              className="size-7 cursor-pointer text-muted-foreground hover:bg-primary/10 hover:text-primary"
              aria-label={t('skills.actions.copyToProject')}
              title={t('skills.actions.copyToProject')}
              disabled={writeBlocked}
              onClick={(event) => {
                event.stopPropagation();
                onCopyToProject(skill);
              }}
            >
              <FolderOutput className="size-3.5" aria-hidden="true" />
            </Button>
          ) : null}
          {onManageAgents ? (
            <Button
              variant="ghost"
              size="icon"
              className="size-7 cursor-pointer text-muted-foreground hover:bg-muted hover:text-foreground"
              aria-label={t('skills.manageAgents.action')}
              title={t('skills.manageAgents.action')}
              disabled={writeBlocked}
              onClick={(event) => {
                event.stopPropagation();
                onManageAgents(skill);
              }}
            >
              <Pencil className="size-3.5" aria-hidden="true" />
            </Button>
          ) : null}
          {onDelete ? (
            <Button
              variant="ghost"
              size="icon"
              className="size-7 cursor-pointer text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
              aria-label={t('skills.actions.delete')}
              title={t('skills.actions.delete')}
              disabled={writeBlocked}
              onClick={(event) => {
                event.stopPropagation();
                onDelete(skill);
              }}
            >
              <Trash2 className="size-3.5" aria-hidden="true" />
            </Button>
          ) : null}
        </div>
      </CardContent>

      {activeUpdatePhase ? (
        <div className="absolute inset-x-0 bottom-0">
          <div className="h-0.5 overflow-hidden bg-primary/15">
            <div ref={progressBarRef} className="h-full bg-primary transition-all duration-500" style={{ width: '10%' }} />
          </div>
        </div>
      ) : null}
      {updateStatus === 'done' ? (
        <div className="absolute inset-x-0 bottom-0 h-0.5 bg-success transition-opacity duration-700" />
      ) : null}
      {updateStatus === 'failed' ? (
        <div className="absolute inset-x-0 bottom-0 h-0.5 bg-destructive" />
      ) : null}
    </Card>
  );
});
