import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { ArrowUpCircle, Package, Trash2 } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { CardContent } from '@/components/ui/card';
import { CrossfadeSwap } from '@/components/ui/crossfade-swap';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import {
  SkillCardAttentionRow,
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
import type { LibrarySkillSummary, SkillUpdateInfo } from '@/bindings';

interface LibrarySkillCardProps {
  skill: LibrarySkillSummary;
  check?: SkillUpdateInfo;
  /** 本次整库更新批次中该成员的阶段或结果。 */
  updateStatus?: SkillUpdateDisplayStatus;
  busy?: boolean;
  libraryInUse?: boolean;
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
  updateStatus,
  busy = false,
  libraryInUse = false,
  onClick,
  onUpdate,
  onRemove,
}: LibrarySkillCardProps) {
  const { t, i18n } = useTranslation();
  const activation = useCardActivation(onClick ? () => onClick(skill.name) : undefined);

  const sourceLabel = skill.source?.trim() || skill.sourceUrl?.trim() || null;
  const updatedAt = skill.updatedAt ? formatSkillCardDate(skill.updatedAt, i18n.language) : null;
  const activeUpdatePhase = isSkillUpdateActive(updateStatus) ? updateStatus : null;

  // 只有"新版本可用"进标题标签。已是最新是默认状态不占位，来源异常走注意行。
  const canShowUpdateAction = check?.status === 'updateAvailable'
    && !updateStatus
    && Boolean(onUpdate);
  const attentionLabels = [
    check?.status === 'deletedUpstream' ? t('skills.card.sourceMissingUpstream') : null,
    check?.status === 'cannotCheck' ? t('skills.card.updateCheckIncomplete') : null,
  ].filter((label): label is string => Boolean(label));

  const statusTransitionKey = activeUpdatePhase
    ? resolveSkillUpdatePhaseI18nKey(activeUpdatePhase)
    : updateStatus === 'done'
      ? 'skills.updateDone'
      : updateStatus === 'failed'
        ? 'skills.updateFailed'
        : check?.status === 'updateAvailable'
          ? 'skills.updateStatusLabel.available'
          : 'none';

  return (
    <SkillCardShell
      onPointerDown={onClick ? activation.onPointerDown : undefined}
      onClick={onClick ? activation.onClick : undefined}
    >
      <CardContent className="grid grid-cols-[1.5rem_minmax(0,1fr)_auto] items-start gap-x-2.5 p-4">
        <SkillCardMarker icon={Package} testId="library-skill-marker" />

        <div className="min-w-0 space-y-2">
          <div data-testid="library-skill-title" className="flex min-w-0 items-center gap-2 overflow-hidden">
            {onClick ? (
              <button
                type="button"
                title={skill.name}
                className="min-w-16 shrink cursor-pointer text-left outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                onClick={(event) => {
                  event.stopPropagation();
                  onClick(skill.name);
                }}
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
                title={skill.pluginName}
                className="max-w-40 min-w-0 shrink-[2] truncate text-xs text-muted-foreground"
              >
                {skill.pluginName}
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
              ) : check?.status === 'updateAvailable' ? (
                <SkillCardStatusLabel label={t('skills.updateStatusLabel.available')} />
              ) : null}
            </CrossfadeSwap>
          </div>

          {skill.description ? (
            <p className="line-clamp-2 text-sm leading-[21px] text-muted-foreground">
              {skill.description}
            </p>
          ) : null}

          {sourceLabel || skill.refName || updatedAt ? (
            <SkillCardMetaRow>
              {sourceLabel ? (
                <span className="inline-flex min-w-0 items-center">
                  <SkillSourceLink label={sourceLabel} url={skill.sourceUrl ?? skill.source} />
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
                      <span tabIndex={0} className="rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/50">
                        {t('skills.updated', { time: updatedAt.short })}
                      </span>
                    </TooltipTrigger>
                    <TooltipContent><p>{updatedAt.full}</p></TooltipContent>
                  </Tooltip>
                </span>
              ) : null}
            </SkillCardMetaRow>
          ) : null}

          <SkillCardAttentionRow labels={attentionLabels} testId="library-skill-attention" />
        </div>

        <div className="flex shrink-0 items-center gap-0.5 pl-1">
          {canShowUpdateAction ? (
            <Button
              variant="ghost"
              size="icon"
              className="size-7 cursor-pointer text-primary hover:bg-primary/10 hover:text-primary"
              aria-label={t('libraries.update')}
              title={t('libraries.update')}
              disabled={busy}
              onClick={(event) => {
                event.stopPropagation();
                onUpdate?.(skill.name);
              }}
            >
              <ArrowUpCircle className="size-4" aria-hidden="true" />
            </Button>
          ) : null}

          {onRemove ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-7 cursor-pointer text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
                  aria-label={t('libraries.removeSkill', { name: skill.name })}
                  // 成员锁定是库特有的约束，用户需要读到原因；
                  // aria-disabled 保留指针事件，禁用状态下 Tooltip 才能触发。
                  aria-disabled={busy || libraryInUse}
                  onClick={(event) => {
                    event.stopPropagation();
                    if (!busy && !libraryInUse) onRemove(skill.name);
                  }}
                >
                  <Trash2 className="size-3.5" aria-hidden="true" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                <p>
                  {libraryInUse
                    ? t('libraries.lockedMembership')
                    : t('libraries.removeSkillTitle', { name: skill.name })}
                </p>
              </TooltipContent>
            </Tooltip>
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
