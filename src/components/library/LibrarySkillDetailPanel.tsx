import { memo, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { ArrowUpCircle, Trash2, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import {
  SkillCardAttentionRow,
  SkillCardStatusLabel,
} from '@/components/skills/card/SkillCardPrimitives';
import {
  DetailBody,
  DetailField,
  DetailSourceLink,
} from '@/components/skills/detail/DetailPrimitives';
import { isOpenableUrl } from '@/lib/skill-card-presentation';
import { formatTime } from '@/lib/utils';
import type { LibrarySkillSummary, SkillUpdateInfo } from '@/bindings';

interface LibrarySkillDetailPanelProps {
  skill: LibrarySkillSummary;
  check?: SkillUpdateInfo;
  content: string | null;
  loading: boolean;
  contentError?: boolean;
  busy?: boolean;
  libraryInUse?: boolean;
  onClose: () => void;
  onUpdate?: (skillName: string) => void;
  onRemove?: (skillName: string) => void;
  onRetry?: () => void;
}

/**
 * 库成员详情。
 *
 * 与 `Skills` 页详情使用同一套版式：标题与操作簇、独立一行的来源链接、无边框的元信息标签
 * 网格、正文。库成员没有安装路径和关联 Agent，元信息改为展示来源侧的坐标。
 */
export const LibrarySkillDetailPanel = memo(function LibrarySkillDetailPanel({
  skill,
  check,
  content,
  loading,
  contentError = false,
  busy = false,
  libraryInUse = false,
  onClose,
  onUpdate,
  onRemove,
  onRetry,
}: LibrarySkillDetailPanelProps) {
  const { t, i18n } = useTranslation();

  // 详情是分栏而不是弹窗，但用户对 Escape 关闭当前面板有同样的预期。
  useEffect(() => {
    const close = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', close);
    return () => window.removeEventListener('keydown', close);
  }, [onClose]);

  const sourceLabel = skill.source?.trim() || skill.sourceUrl?.trim() || null;
  const sourceUrl = isOpenableUrl(skill.sourceUrl)
    ? skill.sourceUrl
    : isOpenableUrl(skill.source) ? skill.source : null;

  const isUpdateAvailable = check?.status === 'updateAvailable';
  const attentionLabels = [
    check?.status === 'deletedUpstream' ? t('skills.card.sourceMissingUpstream') : null,
    check?.status === 'cannotCheck' ? t('skills.card.updateCheckIncomplete') : null,
  ].filter((label): label is string => Boolean(label));

  const hasMetadata = Boolean(skill.updatedAt || skill.pluginName || skill.refName);

  return (
    <div className="flex h-full flex-col overflow-hidden bg-surface">
      <div className="relative min-h-0 flex-1">
        <ScrollArea className="absolute inset-0 h-full w-full">
          <div className="w-full space-y-4 px-6 py-6 sm:px-8 sm:py-6">
            <div className="space-y-3">
              <div className="flex items-start justify-between gap-4">
                <div className="flex min-w-0 flex-1 items-center gap-2">
                  <h2 className="min-w-0 font-heading text-2xl font-extrabold leading-tight tracking-tight text-foreground sm:text-3xl">
                    {skill.name}
                  </h2>
                  {/* 与卡片一致：只有"新版本可用"进标题，已是最新不占位，来源异常走注意行。 */}
                  {isUpdateAvailable ? (
                    <SkillCardStatusLabel label={t('skills.updateStatusLabel.available')} />
                  ) : null}
                </div>

                <div className="flex shrink-0 items-center gap-1 pt-1">
                  {isUpdateAvailable && onUpdate ? (
                    <Button
                      variant="ghost"
                      size="icon"
                      className="size-8 cursor-pointer text-primary hover:bg-primary/10 hover:text-primary"
                      aria-label={t('libraries.update')}
                      title={t('libraries.update')}
                      disabled={busy}
                      onClick={() => onUpdate(skill.name)}
                    >
                      <ArrowUpCircle className="size-4" aria-hidden="true" />
                    </Button>
                  ) : null}

                  {onRemove ? (
                    <Button
                      variant="ghost"
                      size="icon"
                      className="size-8 cursor-pointer text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
                      aria-label={t('libraries.removeSkill', { name: skill.name })}
                      title={libraryInUse
                        ? t('libraries.lockedMembership')
                        : t('libraries.removeSkillTitle', { name: skill.name })}
                      disabled={busy || libraryInUse}
                      onClick={() => onRemove(skill.name)}
                    >
                      <Trash2 className="size-4" aria-hidden="true" />
                    </Button>
                  ) : null}

                  <Button
                    variant="ghost"
                    size="icon"
                    className="size-8 cursor-pointer text-muted-foreground hover:bg-muted hover:text-foreground"
                    onClick={onClose}
                    aria-label={t('common.close')}
                  >
                    <X className="size-4" aria-hidden="true" />
                  </Button>
                </div>
              </div>

              {skill.description ? (
                <p className="max-w-4xl text-sm leading-relaxed text-muted-foreground">
                  {skill.description}
                </p>
              ) : null}

              <SkillCardAttentionRow labels={attentionLabels} />
            </div>

            {sourceLabel ? (
              <DetailSourceLink label={sourceLabel} url={sourceUrl} />
            ) : null}

            {hasMetadata ? (
              <div className="grid grid-cols-2 gap-4 border-b border-border pb-4 md:grid-cols-3">
                {skill.updatedAt ? (
                  <DetailField label={t('skills.detail.updated')}>
                    {formatTime(skill.updatedAt, i18n.language)}
                  </DetailField>
                ) : null}
                {skill.pluginName ? (
                  <DetailField label={t('libraries.pluginName')}>
                    <span className="truncate">{skill.pluginName}</span>
                  </DetailField>
                ) : null}
                {skill.refName ? (
                  <DetailField label={t('libraries.refName')}>
                    <span className="truncate font-mono">{skill.refName}</span>
                  </DetailField>
                ) : null}
              </div>
            ) : null}

            <div className="pb-10">
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
