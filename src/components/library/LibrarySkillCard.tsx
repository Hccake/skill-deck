import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { formatAppError } from '@/utils/format-app-error';
import { skillStatusPresentation } from '@/lib/skill-status-presentation';
import { ArrowUpCircle, Package, RefreshCw, Trash2 } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { CardContent } from '@/components/ui/card';
import { CrossfadeSwap } from '@/components/ui/crossfade-swap';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import {
  SkillCardAttentionRow,
  SkillCardAction,
  SkillCardMarker,
  SkillCardMetaRow,
  SkillCardProgressBar,
  SkillCardShell,
  SkillCardStatusLabel,
  SkillSourceLink,
} from '@/components/skills/card/SkillCardPrimitives';
import { formatSkillCardDate, useCardActivation } from '@/lib/skill-card-presentation';
import {
  isSkillUpdateActive,
  resolveSkillUpdatePhaseI18nKey,
  type SkillUpdateDisplayStatus,
} from '@/stores/skills-utils';
import type { LibrarySkillSummary, SkillUpdateInfo, LibraryUpdateSkillResult } from '@/bindings';
import { canRetryLibraryUpdate } from '@/lib/libraries/update-progress';
import { formatMutationError } from '@/lib/mutation-results';

interface LibrarySkillCardProps {
  skill: LibrarySkillSummary;
  check?: SkillUpdateInfo;
  result?: LibraryUpdateSkillResult;
  /** 本次整库更新批次中该成员的阶段或结果。 */
  updateStatus?: SkillUpdateDisplayStatus;
  busy?: boolean;
  onClick?: (skillName: string) => void;
  onUpdate?: (skillName: string) => void;
  onRemove?: (skillName: string) => void;
}

/**
 * 库成员卡片。
 *
 * 它是 `Skills` 页 `SkillCard` 的子集：沿用同一套栅格、字号、元信息、状态表达和操作样式，
 * 去掉库里不存在的维度——全局与项目的位置区分、关联 Agent、重装、修复来源、复制到项目和
 * 管理 Agent。不引入 `Skills` 页没有的展示元素。
 */
export const LibrarySkillCard = memo(function LibrarySkillCard({
  skill,
  check,
  result,
  updateStatus,
  busy = false,
  onClick,
  onUpdate,
  onRemove,
}: LibrarySkillCardProps) {
  const { t, i18n } = useTranslation();
  const activation = useCardActivation(onClick ? () => onClick(skill.name) : undefined);

  const presentation = skillStatusPresentation(skill, check);
  const sourceLabel = presentation.local ? t('skills.updateStatusLabel.localSource') : presentation.sourceLabel;
  const updatedAt = skill.updatedAt ? formatSkillCardDate(skill.updatedAt, i18n.language) : null;
  const activeUpdatePhase = isSkillUpdateActive(updateStatus) ? updateStatus : null;

  // 只有"新版本可用"进标题标签。已是最新是默认状态不占位，来源异常走注意行。
  const capability = check?.capability ?? skill.updateCapability;
  const canShowUpdateAction = (check?.status === 'updateAvailable' || canRetryLibraryUpdate(result))
    && capability?.canRunUpdate !== false && !activeUpdatePhase
    && Boolean(onUpdate);
  const attentionLabels = [
    result?.error ? formatMutationError(result.error, t) : null,
    presentation.notice && !result?.error ? t(presentation.notice.labelKey) : null,
  ].filter((label): label is string => Boolean(label));
  const noticeDescription = presentation.notice?.error ? formatAppError(presentation.notice.error, t)
    : presentation.notice?.hintKey ? t(presentation.notice.hintKey) : undefined;

  const statusTransitionKey = activeUpdatePhase
    ? resolveSkillUpdatePhaseI18nKey(activeUpdatePhase)
    : updateStatus === 'done'
      ? 'skills.updateDone'
      : updateStatus === 'failed'
        ? 'skills.updateFailed'
        : presentation.available
          ? 'skills.updateStatusLabel.available'
          : 'none';

  return (
    <SkillCardShell
      onPointerDown={onClick ? activation.onPointerDown : undefined}
      onClick={onClick ? activation.onClick : undefined}
    >
      <CardContent className="grid grid-cols-[1.5rem_minmax(0,1fr)] items-start gap-x-2.5 px-3 py-2">
        <SkillCardMarker icon={Package} testId="library-skill-marker" />

        <div className="flex min-w-0 flex-col">
          <div className="flex min-w-0 items-start justify-between gap-2">
            <div data-testid="library-skill-title" className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 overflow-hidden pt-0.5">
              {onClick ? (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <button
                      type="button"
                      data-skill-open
                      className="min-w-0 shrink cursor-pointer text-left outline-none"
                      onClick={(event) => {
                        event.stopPropagation();
                        onClick(skill.name);
                      }}
                    >
                      <h3 className="truncate font-heading text-sm font-semibold leading-5 tracking-normal text-foreground">
                        {skill.name}
                      </h3>
                    </button>
                  </TooltipTrigger>
                  <TooltipContent className="max-w-80 break-all text-wrap data-[state=closed]:animate-none!">{skill.name}</TooltipContent>
                </Tooltip>
              ) : (
                <h3 className="truncate font-heading text-sm font-semibold leading-5 text-foreground">
                  {skill.name}
                </h3>
              )}

              {skill.pluginName ? (
                <span
                  title={skill.pluginName}
                  className="max-w-40 min-w-0 shrink-[2] truncate text-xs text-muted-foreground"
                >
                  {skill.pluginName}
                </span>
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
                ) : presentation.available ? (
                  <SkillCardStatusLabel label={t('skills.updateStatusLabel.available')} />
                ) : null}
              </CrossfadeSwap>
            </div>

            <div className="-mr-1 flex h-6 shrink-0 items-center gap-0.5">
              {canShowUpdateAction ? (
                <SkillCardAction
                  className="size-7 cursor-pointer text-primary hover:bg-primary/10 hover:text-primary"
                  label={t('skills.card.actionLabel', { action: t('libraries.update'), name: skill.name })}
                  disabled={busy}
                  onClick={(event) => {
                    event.stopPropagation();
                    onUpdate?.(skill.name);
                  }}
                >
                  <ArrowUpCircle className="size-4" aria-hidden="true" />
                </SkillCardAction>
              ) : null}

              {onRemove ? (
                <SkillCardAction
                  className="size-7 cursor-pointer text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
                  label={t('libraries.removeSkill', { name: skill.name })}
                  aria-disabled={busy}
                  onClick={(event) => {
                    event.stopPropagation();
                    if (!busy) onRemove(skill.name);
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

          {sourceLabel || skill.refName || updatedAt ? (
            <SkillCardMetaRow className="mt-1.5">
              {sourceLabel ? (
                <span className="inline-flex min-w-0 items-center">
                  {presentation.local ? <span>{sourceLabel}</span> : <SkillSourceLink label={sourceLabel} url={skill.sourceUrl ?? skill.source} hint={presentation.sourceHintKey ? t(presentation.sourceHintKey) : undefined} />}
                </span>
              ) : null}
              {skill.refName ? (
                <span className="inline-flex items-center">
                  <Badge variant="outline" className="px-1.5 py-0 text-xs">
                    {t('skills.refBadge', { ref: skill.refName })}
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

          <SkillCardAttentionRow labels={attentionLabels} description={noticeDescription} testId="library-skill-attention" className="mt-1.5" />
        </div>
      </CardContent>

      <SkillCardProgressBar
        active={Boolean(activeUpdatePhase)}
        outcome={updateStatus === 'done' ? 'done' : updateStatus === 'failed' ? 'failed' : undefined}
      />
    </SkillCardShell>
  );
});
