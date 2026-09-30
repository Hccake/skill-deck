import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { ArrowUpCircle, Trash2, X } from 'lucide-react';
import { ScrollArea } from '@/components/ui/scroll-area';
import {
  SkillCardAttentionRow,
  SkillCardStatusLabel,
} from '@/components/skills/card/SkillCardPrimitives';
import {
  DetailBody,
  DetailAction,
  DetailDescription,
  DetailHeader,
  DetailField,
  DetailSourceLink,
} from '@/components/skills/detail/DetailPrimitives';
import { isOpenableUrl } from '@/lib/skill-card-presentation';
import { formatTime } from '@/lib/utils';
import type { LibrarySkillSummary, SkillUpdateInfo, LibraryUpdateSkillResult } from '@/bindings';
import { canRetryLibraryUpdate } from '@/lib/libraries/update-progress';
import { formatMutationError } from '@/lib/mutation-results';
import { formatAppError } from '@/utils/format-app-error';
import { skillStatusPresentation } from '@/lib/skill-status-presentation';

interface LibrarySkillDetailPanelProps {
  skill: LibrarySkillSummary;
  check?: SkillUpdateInfo;
  result?: LibraryUpdateSkillResult;
  content: string | null;
  loading: boolean;
  contentError?: boolean;
  busy?: boolean;
  onClose: () => void;
  onUpdate?: (skillName: string) => void;
  onRemove?: (skillName: string) => void;
  onRetry?: () => void;
}

/**
 * 库成员详情。
 *
 * 与已安装详情共用字段层级和操作规格，属性展示库成员自己的来源坐标。
 */
export const LibrarySkillDetailPanel = memo(function LibrarySkillDetailPanel({
  skill,
  check,
  result,
  content,
  loading,
  contentError = false,
  busy = false,
  onClose,
  onUpdate,
  onRemove,
  onRetry,
}: LibrarySkillDetailPanelProps) {
  const { t, i18n } = useTranslation();

  const presentation = skillStatusPresentation(skill, check);
  const sourceLabel = presentation.sourceLabel;
  const sourceUrl = isOpenableUrl(skill.sourceUrl)
    ? skill.sourceUrl
    : isOpenableUrl(skill.source) ? skill.source : null;

  const isUpdateAvailable = check?.status === 'updateAvailable';
  const capability = check?.capability ?? skill.updateCapability;
  const canUpdate = capability?.canRunUpdate !== false && (isUpdateAvailable || canRetryLibraryUpdate(result));
  const attentionLabels = [
    result?.error ? formatMutationError(result.error, t) : null,
    presentation.notice && !result?.error ? t(presentation.notice.labelKey) : null,
  ].filter((label): label is string => Boolean(label));
  const noticeDescription = presentation.notice?.error ? formatAppError(presentation.notice.error, t)
    : presentation.notice?.hintKey ? t(presentation.notice.hintKey) : undefined;

  const hasMetadata = Boolean(presentation.local || sourceLabel || skill.updatedAt || skill.pluginName || skill.refName);

  return (
    <div className="skill-detail-layout flex h-full flex-col overflow-hidden bg-surface">
      <DetailHeader name={skill.name} contentState={content ? loading ? 'refreshing' : contentError ? 'stale' : undefined : undefined}>
        {canUpdate && onUpdate ? (
          <DetailAction
            className="text-primary hover:bg-primary/10 hover:text-primary"
            label={t('libraries.update')}
            disabled={busy}
            onClick={() => onUpdate(skill.name)}
          >
            <ArrowUpCircle className="size-4" aria-hidden="true" />
          </DetailAction>
        ) : null}

        {onRemove ? (
          <DetailAction
            className="hover:bg-destructive/10 hover:text-destructive"
            label={t('libraries.removeSkill', { name: skill.name })}
            hint={t('libraries.removeSkillTitle', { name: skill.name })}
            disabled={busy}
            onClick={() => onRemove(skill.name)}
          >
            <Trash2 className="size-4" aria-hidden="true" />
          </DetailAction>
        ) : null}

        <DetailAction
          className="skill-detail-close"
          onClick={onClose}
          label={t('common.close')}
        >
          <X className="size-4" aria-hidden="true" />
        </DetailAction>
      </DetailHeader>
      <div className="relative min-h-0 flex-1">
        <ScrollArea className="absolute inset-0 h-full w-full">
          <div className="skill-detail-content w-full space-y-4">
            {skill.description || presentation.available ? <div className="space-y-2">
              {skill.description ? (
                <DetailDescription key={skill.name}>{skill.description}</DetailDescription>
              ) : null}
              {presentation.available ? <SkillCardStatusLabel label={t('skills.updateStatusLabel.available')} /> : null}

            </div> : null}

            {hasMetadata ? (
              <div className="skill-detail-metadata">
                {presentation.local || sourceLabel ? (
                  <DetailField label={t('skills.detail.source')}>
                    {presentation.local ? t('skills.updateStatusLabel.localSource') : (
                      <DetailSourceLink label={sourceLabel!} url={sourceUrl}
                        hint={presentation.sourceHintKey ? t(presentation.sourceHintKey) : undefined} />
                    )}
                  </DetailField>
                ) : null}
                {skill.updatedAt ? (
                  <DetailField label={t('skills.detail.updated')}>
                    <time className="tabular-nums" dateTime={skill.updatedAt}>{formatTime(skill.updatedAt, i18n.language)}</time>
                  </DetailField>
                ) : null}
                {skill.pluginName ? (
                  <DetailField label={t('libraries.pluginName')}>
                    <span className="block">{skill.pluginName}</span>
                  </DetailField>
                ) : null}
                {skill.refName ? (
                  <DetailField label={t('libraries.refName')}>
                    <span className="block font-mono">{skill.refName}</span>
                  </DetailField>
                ) : null}
              </div>
            ) : null}
            <SkillCardAttentionRow labels={attentionLabels} description={noticeDescription} />

            <div className="border-t border-border/70 pt-4 pb-8">
              <DetailBody
                loading={loading}
                content={content}
                errorMessage={contentError ? t('libraries.contentError') : null}
                onRetry={onRetry}
              />
            </div>
          </div>
        </ScrollArea>
      </div>
    </div>
  );
});
