import { memo, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { ExternalLink, CircleAlert, type LucideIcon } from 'lucide-react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { toast } from 'sonner';
import { cn } from '@/lib/utils';
import { Card } from '@/components/ui/card';
import { isOpenableUrl } from '@/lib/skill-card-presentation';

interface SkillCardShellProps {
  /** 分栏视图需要知道详情属于哪一行。 */
  selected?: boolean;
  className?: string;
  children: ReactNode;
  onPointerDown?: (event: React.PointerEvent<HTMLElement>) => void;
  onClick?: (event: React.MouseEvent<HTMLElement>) => void;
}

/**
 * 两个页面的 Skill 卡片共用的外壳。
 *
 * 根节点保持普通 `div`：卡内有标题、来源和操作按钮，`role="button"` 不允许交互式后代。
 * 键盘路径由卡内的标题按钮承担，聚焦时靠 `:focus-visible` 让整卡显示聚焦环。
 *
 * 只负责容器样式，内容布局（含 `CardContent` 与进度条之类的兄弟节点）由各自的卡片决定。
 */
export const SkillCardShell = memo(function SkillCardShell({
  selected = false,
  className,
  children,
  onPointerDown,
  onClick,
}: SkillCardShellProps) {
  return (
    <Card
      className={cn(
        'group relative gap-0 rounded-xl border border-border bg-card py-0 transition-[border-color,box-shadow] duration-200',
        'hover:border-primary/40 hover:shadow-sm',
        'has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring/50 has-[:focus-visible]:border-primary/40',
        onClick && 'cursor-pointer',
        selected && 'border-primary ring-1 ring-primary/40 bg-accent/20 shadow-sm',
        className,
      )}
      onPointerDown={onPointerDown}
      onClick={onClick}
    >
      {children}
    </Card>
  );
});

interface SkillSourceLinkProps {
  label: string;
  url?: string | null;
  className?: string;
}

/**
 * 来源展示。可打开时是链接，否则是纯文本。
 */
export const SkillSourceLink = memo(function SkillSourceLink({
  label,
  url,
  className,
}: SkillSourceLinkProps) {
  const { t } = useTranslation();
  const openable = isOpenableUrl(url) ? url : null;

  if (!openable) {
    return <span className={cn('max-w-48 truncate', className)} title={label}>{label}</span>;
  }

  return (
    <button
      type="button"
      aria-label={label}
      title={t('skills.externalLink')}
      className={cn(
        'inline-flex min-w-0 cursor-pointer items-center gap-1 font-medium text-primary outline-none',
        'transition-colors hover:text-primary/80 focus-visible:ring-2 focus-visible:ring-ring/50',
        className,
      )}
      onClick={(event) => {
        event.stopPropagation();
        void openUrl(openable).catch((error: unknown) => {
          console.error('Failed to open Skill source:', error);
          toast.error(t('skills.card.sourceOpenFailed'));
        });
      }}
    >
      <span className="max-w-48 truncate">{label}</span>
      <ExternalLink className="size-3 shrink-0" aria-hidden="true" />
    </button>
  );
});

/**
 * 标题行的更新状态标签。
 *
 * 只表达需要用户注意的状态。"已是最新"是默认背景，不占位；来源异常走注意行，不进标题。
 */
export const SkillCardStatusLabel = memo(function SkillCardStatusLabel({
  label,
  tone = 'accent',
}: {
  label: string;
  tone?: 'accent' | 'muted' | 'warning';
}) {
  return (
    <span className={cn(
      'inline-flex h-5 shrink-0 items-center rounded-sm px-1.5 text-[11px] font-medium',
      tone === 'accent' ? 'bg-primary/10 text-primary'
        : tone === 'muted' ? 'bg-muted text-muted-foreground'
          : 'bg-warning/10 text-warning',
    )}>
      {label}
    </span>
  );
});

/**
 * 需要用户注意但不阻断使用的情况，例如来源已删除或更新检查未完成。
 */
export const SkillCardAttentionRow = memo(function SkillCardAttentionRow({
  labels,
  testId,
}: {
  labels: readonly string[];
  testId?: string;
}) {
  if (labels.length === 0) return null;
  return (
    <div
      data-testid={testId}
      role="note"
      aria-label={labels.join('，')}
      className="flex items-start gap-1.5 rounded-sm text-xs leading-5 text-warning/90 outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
    >
      <CircleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
      <div className="flex min-w-0 flex-wrap gap-x-1.5">
        {labels.map((label, index) => (
          <span key={label} className="inline-flex max-w-full whitespace-normal break-words">
            {index > 0 ? <span className="mr-1.5 text-warning/50" aria-hidden="true">·</span> : null}
            {label}
          </span>
        ))}
      </div>
    </div>
  );
});

/**
 * 卡片左侧的标识列。
 *
 * 固定 1.5rem 宽，让两个页面的卡片在名称、描述和元信息的起始位置上对齐。
 */
export const SkillCardMarker = memo(function SkillCardMarker({
  icon: Icon,
  testId,
}: {
  icon: LucideIcon;
  testId?: string;
}) {
  return (
    <div
      data-testid={testId}
      className="flex size-6 items-center justify-center rounded bg-muted/60 text-muted-foreground ring-1 ring-inset ring-border/50"
    >
      <Icon className="size-3.5 text-foreground/70" aria-hidden="true" />
    </div>
  );
});

/**
 * 卡片底部的更新状态条。执行中显示进度轨道，结束后用一条细线表达结果。
 */
export const SkillCardProgressBar = memo(function SkillCardProgressBar({
  active,
  outcome,
}: {
  active: boolean;
  outcome?: 'done' | 'failed';
}) {
  if (active) {
    return (
      <div className="absolute inset-x-0 bottom-0">
        <div className="h-0.5 overflow-hidden bg-primary/15">
          <div className="h-full bg-primary transition-[width] duration-500" style={{ width: '10%' }} />
        </div>
      </div>
    );
  }
  if (outcome === 'done') {
    return <div className="absolute inset-x-0 bottom-0 h-0.5 bg-success transition-opacity duration-700" />;
  }
  if (outcome === 'failed') {
    return <div className="absolute inset-x-0 bottom-0 h-0.5 bg-destructive" />;
  }
  return null;
});

/**
 * 元信息行。用 `·` 分隔任意数量的片段，空片段自动省略。
 */
export const SkillCardMetaRow = memo(function SkillCardMetaRow({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      data-testid="skill-card-meta"
      className={cn(
        'flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground',
        "[&>span:not(:last-child)]:after:ml-2 [&>span:not(:last-child)]:after:text-border [&>span:not(:last-child)]:after:content-['·']",
        className,
      )}
    >
      {children}
    </div>
  );
});
