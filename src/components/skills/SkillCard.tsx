import { memo, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { formatAppError } from '@/utils/format-app-error';
import {
  ArrowUpCircle,
  Folder,
  FolderOutput,
  Globe,
  Bot,
  RefreshCw,
  Trash2,
} from 'lucide-react';
import { toTitleCase } from '@/lib/utils';
import { skillStatusPresentation } from '@/lib/skill-status-presentation';
import { getSkillIdentity, getSkillIdentityKey } from '@/lib/skills/identity';
import { formatSkillCardDate, isOpenableUrl, useCardActivation } from '@/lib/skill-card-presentation';
import { Badge } from '@/components/ui/badge';
import { CardContent } from '@/components/ui/card';
import {
  SkillCardMarker,
  SkillCardAction,
  SkillAgentBadge,
  SkillCardMetaRow,
  SkillCardProgressBar,
  SkillCardShell,
  SkillCardStatusLabel,
  SkillCardAttentionRow,
  SkillSourceLink,
} from '@/components/skills/card/SkillCardPrimitives';
import { CrossfadeSwap } from '@/components/ui/crossfade-swap';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { OverflowTooltip } from '@/components/ui/overflow-tooltip';
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
  projectPath,
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

  const attentionRow = <SkillCardAttentionRow labels={attentionLabels} description={attentionTitle} testId="skill-card-attention" className="mt-1.5" />;

  return (
    <SkillCardShell
      itemKey={getSkillIdentityKey(getSkillIdentity(skill, projectPath))}
      onPointerDown={onClick ? activation.onPointerDown : undefined}
      onClick={onClick ? activation.onClick : undefined}
    >
      <CardContent className="grid grid-cols-[1.5rem_minmax(0,1fr)] items-start gap-x-2.5 px-3 py-2">
        <SkillCardMarker icon={ScopeIcon} testId="skill-scope-marker" />

        <div className="flex min-w-0 flex-col">
          <div className="flex min-w-0 items-start justify-between gap-2">
            <div data-testid="skill-card-title" className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 overflow-hidden pt-0.5">
              {onClick ? (
                <OverflowTooltip content={skill.name}>
                    <button
                      type="button"
                      data-skill-open
                      className="min-w-0 shrink cursor-pointer text-left outline-none"
                      onClick={handleTitleClick}
                    >
                      <h3 className="truncate font-heading text-sm font-semibold leading-5 tracking-normal text-foreground">
                        {skill.name}
                      </h3>
                    </button>
                </OverflowTooltip>
              ) : (
                <h3 className="truncate font-heading text-sm font-semibold leading-5 text-foreground">
                  {skill.name}
                </h3>
              )}
              {skill.pluginName ? (
                <OverflowTooltip content={toTitleCase(skill.pluginName)}><span tabIndex={0}
                  className="max-w-40 min-w-0 shrink-[2] truncate text-xs text-muted-foreground"
                >
                  {toTitleCase(skill.pluginName)}
                </span></OverflowTooltip>
              ) : null}
              <CrossfadeSwap transitionKey={statusTransitionKey} className="shrink-0">
                {activeUpdatePhase ? (
                  <Badge variant="outline" className="shrink-0 text-xs text-primary">
                    <RefreshCw className="size-3 motion-safe:animate-spin" aria-hidden="true" />
                    {t(resolveSkillUpdatePhaseI18nKey(activeUpdatePhase))}
                  </Badge>
                ) : updateStatus === 'done' ? (
                  <Badge variant="outline" className="shrink-0 text-xs text-emerald-700 dark:text-success">{t('skills.updateDone')}</Badge>
                ) : updateStatus === 'failed' ? (
                  <Badge variant="outline" className="shrink-0 text-xs text-red-700 dark:text-destructive">{t('skills.updateFailed')}</Badge>
                ) : titleStatusLabelKey ? (
                  <SkillCardStatusLabel label={t(titleStatusLabelKey)} />
                ) : null}
              </CrossfadeSwap>
            </div>

            <div className="-mr-1 flex h-6 shrink-0 items-center gap-0.5">
              {canShowUpdateAction ? (
                <SkillCardAction
                  className="size-7 cursor-pointer text-primary hover:bg-primary/10 hover:text-primary"
                  label={t('skills.actions.updateNamed', { name: skill.name })}
                  disabled={writeBlocked}
                  onClick={(event) => {
                    event.stopPropagation();
                    onUpdate?.(skill.name);
                  }}
                >
                  <ArrowUpCircle className="size-4" aria-hidden="true" />
                </SkillCardAction>
              ) : null}
              {displayScope === 'project' && onCopyToProject ? (
                <SkillCardAction
                  className="size-7 cursor-pointer text-muted-foreground hover:bg-primary/10 hover:text-primary"
                  label={t('skills.actions.copyToProjectNamed', { name: skill.name })}
                  disabled={writeBlocked}
                  onClick={(event) => {
                    event.stopPropagation();
                    onCopyToProject(skill);
                  }}
                >
                  <FolderOutput className="size-3.5" aria-hidden="true" />
                </SkillCardAction>
              ) : null}
              {onManageAgents ? (
                <SkillCardAction
                  className="size-7 cursor-pointer text-muted-foreground hover:bg-muted hover:text-foreground"
                  label={t('skills.manageAgents.title', { name: skill.name })}
                  disabled={writeBlocked}
                  onClick={(event) => {
                    event.stopPropagation();
                    onManageAgents(skill);
                  }}
                >
                  <Bot className="size-3.5" aria-hidden="true" />
                </SkillCardAction>
              ) : null}
              {onDelete ? (
                <SkillCardAction
                  className="size-7 cursor-pointer text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
                  label={t('skills.actions.deleteNamed', { name: skill.name })}
                  disabled={writeBlocked}
                  onClick={(event) => {
                    event.stopPropagation();
                    onDelete(skill);
                  }}
                >
                  <Trash2 className="size-3.5" aria-hidden="true" />
                </SkillCardAction>
              ) : null}
            </div>
          </div>

          {skill.description ? (
            <p className="mt-0.5 line-clamp-2 text-xs leading-[18px] text-muted-foreground">
              {skill.description}
            </p>
          ) : null}

          {attentionRow}

          {sourceLabel || skill.gitRef || updatedAt ? (
            <SkillCardMetaRow className="mt-1.5">
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
                      <span tabIndex={0} className="rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring">
                        {t('skills.updated', { time: updatedAt.short })}
                      </span>
                    </TooltipTrigger>
                    <TooltipContent><p>{updatedAt.full}</p></TooltipContent>
                  </Tooltip>
                </span>
              ) : null}
            </SkillCardMetaRow>
          ) : null}

          {effectiveAgents.length > 0 ? (
            <div className="mt-2 flex min-w-0 flex-wrap items-start gap-x-1.5 gap-y-1">
              {effectiveAgents.map((agentId) => (
                <SkillAgentBadge key={agentId}>
                  {agentDisplayNames.get(agentId) ?? agentId}
                </SkillAgentBadge>
              ))}
            </div>
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
