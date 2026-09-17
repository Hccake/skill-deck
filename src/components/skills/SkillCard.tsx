import { memo, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { formatAppError } from '@/utils/format-app-error';
import {
  ArrowUpCircle,
  Folder,
  FolderOutput,
  Globe,
  Pencil,
  Trash2,
} from 'lucide-react';
import { toTitleCase } from '@/lib/utils';
import { skillStatusPresentation } from '@/lib/skill-status-presentation';
import { formatSkillCardDate, isOpenableUrl, useCardActivation } from '@/lib/skill-card-presentation';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { CardContent } from '@/components/ui/card';
import {
  SkillCardMarker,
  SkillCardProgressBar,
  SkillCardShell,
  SkillCardStatusLabel,
  SkillCardAttentionRow,
  SkillSourceLink,
} from '@/components/skills/card/SkillCardPrimitives';
import { CrossfadeSwap } from '@/components/ui/crossfade-swap';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type { AgentId, InstalledSkill, InstalledSkillLocation } from '@/bindings';
import {
  isSkillUpdateActive,
  resolveSkillUpdatePhaseI18nKey,
  type SkillListItem,
  type SkillUpdateDisplayStatus,
} from '@/stores/skills-utils';

const EMPTY_DISPLAY_NAMES = new Map<AgentId, string>();

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
}: SkillCardProps) {
  const { t, i18n } = useTranslation();
  const effectiveAgents = Array.from(new Set(skill.associatedAgents));
  const scopeIcon = displayScope === 'global' ? Globe : Folder;
  const ScopeIcon = scopeIcon;
  const deletedUpstream = skill.updateStatus === 'deletedUpstream' || ['deletedUpstream', 'deleted-upstream'].includes(skill.updateReason ?? '');
  const presentation = skillStatusPresentation(skill);
  const canShowUpdateAction = skill.hasUpdate === true
    && skill.canRunUpdate !== false
    && !deletedUpstream
    && !updateStatus
    && Boolean(onUpdate);
  const activeUpdatePhase = isSkillUpdateActive(updateStatus) ? updateStatus : null;
  const titleStatusLabelKey = presentation.available ? 'skills.updateStatusLabel.available' : null;
  const statusTransitionKey = activeUpdatePhase
    ? resolveSkillUpdatePhaseI18nKey(activeUpdatePhase)
    : updateStatus === 'done'
      ? 'skills.updateDone'
      : updateStatus === 'failed'
        ? 'skills.updateFailed'
        : titleStatusLabelKey ?? 'none';
  const notice = presentation.notice;
  const attentionLabels = [
    notice ? t(notice.labelKey) : null,
    hasDuplicateLocation ? t('skills.card.duplicateLocations') : null,
    (skill.duplicateCopyCount ?? 0) > 0 ? t('skills.card.duplicateAgentInstall') : null,
  ].filter((label): label is string => Boolean(label));
  const attentionTitle = notice?.error ? formatAppError(notice.error, t)
    : notice?.hintKey ? t(notice.hintKey) : undefined;
  const sourceLabel = presentation.local ? t('skills.updateStatusLabel.localSource') : presentation.sourceLabel;
  const sourceUrl = isOpenableUrl(skill.sourceUrl) ? skill.sourceUrl : null;
  const updatedAt = skill.updatedAt
    ? formatSkillCardDate(skill.updatedAt, i18n.language)
    : null;
  const activation = useCardActivation(onClick ? () => onClick(skill) : undefined);

  const handleTitleClick = useCallback((event: React.MouseEvent<HTMLButtonElement>) => {
    event.stopPropagation();
    onClick?.(skill);
  }, [onClick, skill]);

  const attentionRow = <SkillCardAttentionRow labels={attentionLabels} description={attentionTitle} testId="skill-card-attention" />;

  return (
    <SkillCardShell
      onPointerDown={onClick ? activation.onPointerDown : undefined}
      onClick={onClick ? activation.onClick : undefined}
    >
      <CardContent className="grid grid-cols-[1.5rem_minmax(0,1fr)_auto] items-start gap-x-2.5 p-4">
        <Tooltip>
          <TooltipTrigger asChild>
            <div>
              <SkillCardMarker icon={ScopeIcon} testId="skill-scope-marker" />
            </div>
          </TooltipTrigger>
          <TooltipContent><p>{t(`skills.scopeIcon.${displayScope}`)}</p></TooltipContent>
        </Tooltip>

        <div className="min-w-0 space-y-2">
          <div data-testid="skill-card-title" className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 overflow-hidden">
            {onClick ? (
              <button
                type="button"
                title={skill.name}
                className="min-w-0 shrink cursor-pointer text-left outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
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
                <SkillCardStatusLabel label={t(titleStatusLabelKey)} />
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
                  {presentation.local ? <span>{sourceLabel}</span> : (
                    <SkillSourceLink label={sourceLabel} url={sourceUrl} hint={presentation.sourceHintKey ? t(presentation.sourceHintKey) : undefined} />
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

          {attentionRow}

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

      <SkillCardProgressBar
        active={Boolean(activeUpdatePhase)}
        outcome={updateStatus === 'done' ? 'done' : updateStatus === 'failed' ? 'failed' : undefined}
      />
    </SkillCardShell>
  );
});
