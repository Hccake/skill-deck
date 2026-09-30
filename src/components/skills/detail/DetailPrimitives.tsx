import { memo, useCallback, useEffect, useId, useRef, useState, type ComponentProps, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { AlertTriangle, Check, ChevronDown, ChevronUp, Copy, Link2, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { openUrl } from '@tauri-apps/plugin-opener';
import { toast } from 'sonner';
import { isOpenableUrl } from '@/lib/skill-card-presentation';
import { cn } from '@/lib/utils';
import './SkillDetail.css';

export function DetailHeader({ name, contentState, children }: {
  name: string;
  contentState?: 'refreshing' | 'stale';
  children: ReactNode;
}) {
  const { t } = useTranslation();
  const nameRef = useRef<HTMLHeadingElement>(null);
  const [truncated, setTruncated] = useState(false);
  useEffect(() => {
    const element = nameRef.current;
    if (!element) return;
    const measure = () => setTruncated(element.scrollWidth > element.clientWidth);
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    measure();
    return () => observer.disconnect();
  }, [name]);

  return (
    <div className="skill-detail-header">
      <div className="skill-detail-heading flex items-center gap-2">
        <Tooltip>
          <TooltipTrigger asChild>
            <h2 ref={nameRef} tabIndex={truncated ? 0 : undefined}
              className="min-w-0 flex-1 truncate rounded-sm font-heading text-base font-semibold leading-6 text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring">
              {name}
            </h2>
          </TooltipTrigger>
          {truncated ? <TooltipContent className="max-w-96 break-all text-wrap data-[state=closed]:animate-none!">{name}</TooltipContent> : null}
        </Tooltip>
        {contentState ? <Tooltip>
          <TooltipTrigger asChild>
            <span tabIndex={0} role="img" aria-label={t(contentState === 'stale' ? 'skills.detail.refreshFailed' : 'skills.detail.refreshing')}
              className="shrink-0 rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring">
              {contentState === 'stale'
                ? <AlertTriangle className="size-3.5 text-warning" aria-hidden="true" />
                : <RefreshCw className="size-3.5 animate-spin text-muted-foreground motion-reduce:animate-none" aria-hidden="true" />}
            </span>
          </TooltipTrigger>
          <TooltipContent>{t(contentState === 'stale' ? 'skills.detail.refreshFailed' : 'skills.detail.refreshing')}</TooltipContent>
        </Tooltip> : null}
      </div>
      <div className="skill-detail-actions">{children}</div>
    </div>
  );
}

/**
 * 详情面板中的属性字段。
 *
 * 标签与值作为整体排布，字段宽度由内容及可用空间决定。
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
    <dl className={cn('skill-detail-field flex min-w-0 max-w-full flex-col', className)}>
      <dt className="text-xs font-normal leading-[18px] tracking-normal text-muted-foreground">
        {label}
      </dt>
      <dd className="mt-1 min-w-0 text-[13px] font-normal leading-5 text-foreground [overflow-wrap:anywhere]">{children}</dd>
    </dl>
  );
});

export function DetailDescription({ children }: { children: string }) {
  const { t } = useTranslation();
  const id = useId();
  const textRef = useRef<HTMLParagraphElement>(null);
  const [expanded, setExpanded] = useState(false);
  const [overflows, setOverflows] = useState(false);

  useEffect(() => {
    const element = textRef.current;
    if (!element) return;
    const measure = () => setOverflows(element.scrollHeight > (parseFloat(getComputedStyle(element).lineHeight) || 22) * 2 + 1);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [children]);

  return (
    <div className="flex items-start gap-1">
      <p ref={textRef} id={id} className={cn('min-w-0 flex-1 text-sm leading-[22px] text-foreground/80 [overflow-wrap:anywhere]', !expanded && 'line-clamp-2')}>
        {children}
      </p>
      {overflows ? (
        <DetailAction className="size-7" label={t(expanded ? 'skills.detail.collapseDescription' : 'skills.detail.expandDescription')}
          aria-expanded={expanded} aria-controls={id} onClick={() => setExpanded(value => !value)}>
          {expanded ? <ChevronUp className="size-4" aria-hidden="true" /> : <ChevronDown className="size-4" aria-hidden="true" />}
        </DetailAction>
      ) : null}
    </div>
  );
}

export function DetailAction({ label, hint, className, disabled, ...props }: ComponentProps<typeof Button> & { label: string; hint?: string }) {
  const button = <Button variant="ghost" size="icon" {...props} disabled={disabled} aria-label={label}
    className={cn('size-8 shrink-0 cursor-pointer text-muted-foreground hover:bg-muted/50 hover:text-foreground', className)} />;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span tabIndex={disabled && hint ? 0 : undefined} aria-label={disabled ? hint : undefined}
          className="inline-flex shrink-0 rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring">{button}</span>
      </TooltipTrigger>
      <TooltipContent className="max-w-80 text-wrap data-[state=closed]:animate-none!">{hint ?? label}</TooltipContent>
    </Tooltip>
  );
}

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

  if (content) {
    return (
      <div>
        {loading ? <p role="status" className="mb-3 text-xs leading-5 text-muted-foreground">{t('skills.detail.refreshing')}</p> : null}
        {!loading && errorMessage ? (
          <div role="alert" className="mb-3 flex items-start gap-2 text-xs leading-5 text-foreground">
            <div className="min-w-0 flex-1">
              <p>{errorMessage}</p>
              <p className="text-muted-foreground">{t('skills.detail.showingPreviousContent')}</p>
            </div>
            {onRetry ? <Button variant="outline" size="sm" onClick={onRetry}><RefreshCw className="size-3.5" aria-hidden="true" />{t('skills.detail.retry')}</Button> : null}
          </div>
        ) : null}
        <div className="skill-prose skill-prose-with-lists" aria-busy={loading}>
          <MarkdownContent content={content} />
        </div>
      </div>
    );
  }

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

  return (
    <div className="flex flex-col items-center justify-center py-8 text-muted-foreground">
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
  hint,
}: {
  label: string;
  url?: string | null;
  hint?: string;
}) {
  const { t } = useTranslation();
  const openable = isOpenableUrl(url) ? url : null;

  const source = !openable ? (
      <span tabIndex={0} className="skill-detail-source inline-flex min-w-0 max-w-full items-center gap-1.5 rounded-sm text-[13px] leading-5 text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50">
        <span className="truncate">{label}</span>
        <Link2 className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
      </span>
  ) : (
    <button
      type="button"
      className="skill-detail-source inline-flex min-w-0 max-w-full cursor-pointer items-center gap-1.5 rounded-sm text-[13px] font-medium leading-5 text-primary outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring/50"
      onClick={() => {
        void openUrl(openable).catch((error: unknown) => {
          console.error('Failed to open Skill source:', error);
          toast.error(t('skills.card.sourceOpenFailed'));
        });
      }}
    >
      <span className="truncate">{label}</span>
      <Link2 className="size-3.5 shrink-0" aria-hidden="true" />
    </button>
  );
  return <Tooltip><TooltipTrigger asChild>{source}</TooltipTrigger>
    <TooltipContent className="max-w-[min(32rem,calc(100vw-2rem))] break-all text-wrap data-[state=closed]:animate-none!">
      <p>{label}</p>
      {hint ? <p className="mt-1">{hint}</p> : null}
    </TooltipContent></Tooltip>;
});
