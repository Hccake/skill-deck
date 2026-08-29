import { memo, useCallback, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Check, Copy, Link2, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { openUrl } from '@tauri-apps/plugin-opener';
import { toast } from 'sonner';
import { isOpenableUrl } from '@/lib/skill-card-presentation';
import { cn } from '@/lib/utils';

/**
 * 详情面板元信息网格中的一格。
 *
 * 标签使用统一的小型大写排版；缺值的格由调用方省略，网格自然收缩。
 */
export const DetailField = memo(function DetailField({
  label,
  className,
  children,
}: {
  label: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div className={cn('flex flex-col', className)}>
      <span className="font-heading text-[10px] font-bold uppercase tracking-[0.2em] text-muted-foreground">
        {label}
      </span>
      <div className="mt-1 text-sm font-semibold text-accent-foreground">{children}</div>
    </div>
  );
});

/**
 * 可复制的路径值。
 *
 * 截断展示，聚焦或悬停时通过 Tooltip 查看完整值；复制成功后短暂显示对勾再回落。
 */
export const CopyablePath = memo(function CopyablePath({ value }: { value: string }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      console.error('Failed to copy path');
    }
  }, [value]);

  return (
    <div className="flex min-w-0 items-center gap-1">
      <Tooltip>
        <TooltipTrigger asChild>
          <code
            tabIndex={0}
            className="min-w-0 flex-1 truncate bg-sidebar px-2 py-1 font-mono text-sm text-accent-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
          >
            {value}
          </code>
        </TooltipTrigger>
        <TooltipContent className="max-w-[min(32rem,calc(100vw-2rem))] text-wrap break-all text-left">
          <p className="font-mono">{value}</p>
        </TooltipContent>
      </Tooltip>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant="ghost"
            size="icon-xs"
            className="size-5 shrink-0 border-none text-muted-foreground shadow-none hover:bg-muted/50"
            onClick={() => void handleCopy()}
            aria-label={t('common.copy')}
          >
            {copied
              ? <Check className="size-3 text-success" aria-hidden="true" />
              : <Copy className="size-3" aria-hidden="true" />}
          </Button>
        </TooltipTrigger>
        <TooltipContent><p>{t('common.copy')}</p></TooltipContent>
      </Tooltip>
    </div>
  );
});

const MarkdownContent = memo(function MarkdownContent({ content }: { content: string }) {
  return <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>;
});

/**
 * 详情正文的三种状态：读取中、读取失败或没有内容、以及渲染 `SKILL.md`。
 *
 * 失败与空内容都提供重试，因为两者的下一步是同一个动作。
 */
export const DetailBody = memo(function DetailBody({
  loading,
  content,
  errorMessage,
  onRetry,
}: {
  loading: boolean;
  content: string | null;
  errorMessage?: string | null;
  onRetry?: () => void;
}) {
  const { t } = useTranslation();

  if (loading) {
    return (
      <div className="space-y-4">
        <Skeleton className="h-6 w-1/3" />
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-5/6" />
        <Skeleton className="h-4 w-11/12" />
        <Skeleton className="mt-6 h-32 w-full" />
      </div>
    );
  }

  if (content) {
    return (
      <div className="skill-prose skill-prose-with-lists">
        <MarkdownContent content={content} />
      </div>
    );
  }

  return (
    <div className="flex flex-col items-center justify-center py-20 text-muted-foreground">
      <p className={cn('pb-4 text-sm', errorMessage && 'text-destructive')}>
        {errorMessage ?? t('skills.detail.emptyContent')}
      </p>
      {onRetry ? (
        <Button variant="outline" size="sm" onClick={onRetry} className="bg-transparent">
          <RefreshCw className="mr-2 size-3.5" aria-hidden="true" />
          {t('skills.detail.retry')}
        </Button>
      ) : null}
    </div>
  );
});

/**
 * 详情面板中的来源链接。
 *
 * 桌面应用里 `<a target="_blank">` 在 WebView 中不会打开系统浏览器，必须走 Tauri opener
 * 交给系统默认应用。地址不可打开时退化为纯文本。
 */
export const DetailSourceLink = memo(function DetailSourceLink({
  label,
  url,
}: {
  label: string;
  url?: string | null;
}) {
  const { t } = useTranslation();
  const openable = isOpenableUrl(url) ? url : null;

  if (!openable) {
    return (
      <span className="inline-flex min-w-0 max-w-full items-center gap-1.5 text-sm text-muted-foreground">
        <Link2 className="size-3.5 shrink-0" aria-hidden="true" />
        <span className="truncate">{label}</span>
      </span>
    );
  }

  return (
    <button
      type="button"
      title={t('skills.externalLink')}
      className="inline-flex min-w-0 max-w-full cursor-pointer items-center gap-1.5 rounded-sm text-sm font-medium text-primary outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring/50"
      onClick={() => {
        void openUrl(openable).catch((error: unknown) => {
          console.error('Failed to open Skill source:', error);
          toast.error(t('skills.card.sourceOpenFailed'));
        });
      }}
    >
      <Link2 className="size-3.5 shrink-0" aria-hidden="true" />
      <span className="truncate">{label}</span>
    </button>
  );
});
