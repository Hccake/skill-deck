import { memo, type ComponentProps, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { ArrowUpCircle, ExternalLink, CircleAlert, type LucideIcon } from 'lucide-react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { toast } from 'sonner';
import { cn } from '@/lib/utils';
import { Card } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { isOpenableUrl } from '@/lib/skill-card-presentation';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';

interface SkillCardShellProps {
  itemKey?: string;
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
 * 名称入口聚焦时强调整卡，来源与行内命令只强调各自控件。
 *
 * 只负责容器样式，内容布局（含 `CardContent` 与进度条之类的兄弟节点）由各自的卡片决定。
 */
export const SkillCardShell = memo(function SkillCardShell({
  itemKey,
  selected = false,
  className,
  children,
  onPointerDown,
  onClick,
}: SkillCardShellProps) {
  return (
    <Card
      data-skill-item={itemKey}
      className={cn(
        'group relative gap-0 rounded-xl border border-border bg-card py-0 shadow-none transition-colors duration-150',
        'hover:border-foreground/20',
        'has-[[data-skill-open]:focus-visible]:ring-2 has-[[data-skill-open]:focus-visible]:ring-ring has-[[data-skill-open]:focus-visible]:border-primary',
        onClick && 'cursor-pointer',
        selected && 'border-primary ring-1 ring-primary/40 bg-primary/5',
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
  hint?: string;
}

/**
 * 来源展示。可打开时是链接，否则是纯文本。
 */
export const SkillSourceLink = memo(function SkillSourceLink({
  label,
  url,
  className,
  hint,
}: SkillSourceLinkProps) {
  const { t } = useTranslation();
  const openable = isOpenableUrl(url) ? url : null;

  const source = !openable ? (
    <span className={cn('block max-w-48 truncate outline-none focus-visible:ring-2 focus-visible:ring-ring', className)} tabIndex={0}>{label}</span>
  ) : (
    <button
      type="button"
      aria-label={label}
      className={cn(
        'inline-flex min-w-0 cursor-pointer items-center gap-1 font-medium text-primary outline-none',
        'hover:underline focus-visible:ring-2 focus-visible:ring-ring',
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
  return <Tooltip><TooltipTrigger asChild>{source}</TooltipTrigger>
    <TooltipContent className="max-w-80 break-all text-wrap data-[state=closed]:animate-none!">
      <p>{label}</p>
      {hint ? <p className="mt-1">{hint}</p> : null}
    </TooltipContent></Tooltip>;
});

export function SkillCardAction({ label, className, ...props }: ComponentProps<typeof Button> & { label: string }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="ghost" size="icon" {...props} aria-label={label} className={cn('size-7 cursor-pointer text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring', className)} />
      </TooltipTrigger>
      <TooltipContent className="data-[state=closed]:animate-none!">{label}</TooltipContent>
    </Tooltip>
  );
}

export function SkillAgentBadge({ children }: { children: ReactNode }) {
  return (
    <span className="skill-agent-badge inline-flex min-h-5 max-w-full items-center rounded-full bg-primary/6 px-2 py-0.5 text-xs leading-4 font-medium text-[color:color-mix(in_srgb,var(--primary)_55%,var(--muted-foreground))] ring-1 ring-inset ring-primary/12 dark:bg-primary/8 dark:ring-primary/14 [overflow-wrap:anywhere]">
      {children}
    </span>
  );
}

export const SkillCompactRow = memo(function SkillCompactRow({ name, source, selected, onClick, itemKey }: {
  itemKey?: string;
  name: string;
  source: string | null;
  selected: boolean;
  onClick: () => void;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          data-skill-open
          data-skill-item={itemKey}
          aria-current={selected ? 'true' : undefined}
          className={cn(
            'group relative block min-h-14 w-full cursor-pointer overflow-hidden px-4 py-2 text-left transition-colors outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring',
            selected ? 'bg-primary/8 shadow-[inset_2px_0_0_0_theme(colors.primary.DEFAULT)]' : 'hover:bg-accent/40',
          )}
          onClick={onClick}
        >
          <div className={cn('truncate font-heading text-sm font-semibold leading-5 tracking-normal', selected ? 'text-primary' : 'text-foreground')}>{name}</div>
          <div className="mt-0.5 min-h-[18px] truncate text-xs leading-[18px] text-zinc-600 dark:text-muted-foreground">{source}</div>
        </button>
      </TooltipTrigger>
      <TooltipContent className="max-w-80 break-all text-wrap data-[state=closed]:animate-none!">
        <p className="font-medium">{name}</p>
        {source ? <p className="mt-1">{source}</p> : null}
      </TooltipContent>
    </Tooltip>
  );
});

/**
 * 标题行的更新状态标签。
 *
 * 展示已确认的更新机会；来源属性和需要处理的问题分别展示。
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
      'inline-flex min-h-5.5 shrink-0 items-center gap-1 rounded-sm px-1.5 text-xs leading-[18px] font-medium',
      tone === 'accent' ? 'bg-primary/10 text-primary'
        : tone === 'muted' ? 'bg-muted text-muted-foreground'
          : 'bg-warning/10 text-amber-800 dark:text-warning',
    )}>
      {tone === 'accent' ? <ArrowUpCircle className="size-3 shrink-0" aria-hidden="true" /> : null}
      {label}
    </span>
  );
});

/**
 * 简短来源提醒及安装关系，必要的原因支持按需查看。
 */
export const SkillCardAttentionRow = memo(function SkillCardAttentionRow({
  labels,
  testId,
  description,
  className,
}: {
  labels: readonly string[];
  testId?: string;
  description?: string;
  className?: string;
}) {
  if (labels.length === 0) return null;
  const row = (
    <div
      data-testid={testId}
      role="note"
      aria-label={labels.join('，')}
      tabIndex={description ? 0 : undefined}
      className={cn('flex items-start gap-1.5 rounded-sm text-xs leading-[18px] text-amber-800 dark:text-warning outline-none focus-visible:ring-2 focus-visible:ring-ring', className)}
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
  return description ? <Tooltip><TooltipTrigger asChild>{row}</TooltipTrigger>
    <TooltipContent className="max-w-80 whitespace-pre-line text-wrap data-[state=closed]:animate-none!">{description}</TooltipContent></Tooltip> : row;
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
        'flex max-w-full flex-wrap items-center gap-x-2 gap-y-1 text-xs leading-[18px] text-muted-foreground',
        "[&>span:not(:last-child)]:after:ml-2 [&>span:not(:last-child)]:after:text-border [&>span:not(:last-child)]:after:content-['·']",
        className,
      )}
    >
      {children}
    </div>
  );
});
