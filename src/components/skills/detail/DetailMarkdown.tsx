import { memo, useEffect, useId, useMemo, useRef, useState, type ComponentProps, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import ReactMarkdown, { type Components, type ExtraProps } from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Check, Copy } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { DocumentImage, DocumentLink } from './DetailResources';
import { rehypeDetailDocument, remarkDetailTargets } from './detail-markdown-tree';

function useHorizontalOverflow<T extends HTMLElement>(children: ReactNode) {
  const ref = useRef<T>(null);
  const [overflowing, setOverflowing] = useState(false);
  useEffect(() => {
    const viewport = ref.current;
    if (!viewport) return;
    const measure = () => setOverflowing(viewport.scrollWidth > viewport.clientWidth + 1);
    const observer = new ResizeObserver(measure);
    observer.observe(viewport);
    if (viewport.firstElementChild) observer.observe(viewport.firstElementChild);
    measure();
    return () => observer.disconnect();
  }, [children]);
  return { ref, overflowing };
}

function CodeBlock({ node, children, ...props }: ComponentProps<'pre'> & ExtraProps) {
  const { t } = useTranslation();
  const { ref, overflowing } = useHorizontalOverflow<HTMLPreElement>(children);
  const [copiedText, setCopiedText] = useState<string | null>(null);
  const code = node?.children[0];
  const text = code?.type === 'element'
    ? code.children.filter(child => child.type === 'text').map(child => child.value).join('')
    : '';
  const copied = copiedText === text;
  const multiline = text.replace(/\n$/, '').includes('\n');

  useEffect(() => {
    if (copiedText === null) return;
    const timer = setTimeout(() => setCopiedText(null), 1500);
    return () => clearTimeout(timer);
  }, [copiedText]);

  const copy = async () => {
    setCopiedText(null);
    try {
      await navigator.clipboard.writeText(text);
      setCopiedText(text);
    } catch {
      toast.error(t('skills.detail.copyCodeFailed'));
    }
  };

  const copyButton = (
    <Tooltip key="copy">
      <TooltipTrigger asChild>
        <Button type="button" variant="ghost" size="icon" className="skill-code-copy size-7 text-muted-foreground hover:text-foreground"
          aria-label={t('skills.detail.copyCode')} onClick={() => void copy()}>
          {copied ? <Check className="size-3.5" aria-hidden="true" /> : <Copy className="size-3.5" aria-hidden="true" />}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{t(copied ? 'skills.detail.codeCopied' : 'skills.detail.copyCode')}</TooltipContent>
    </Tooltip>
  );

  return (
    <div className="skill-code-block" data-multiline={multiline}>
      {multiline ? copyButton : null}
      <pre {...props} ref={ref} tabIndex={overflowing ? 0 : -1}
        role={overflowing ? 'region' : undefined} aria-label={overflowing ? t('skills.detail.codeBlock') : undefined}>
        {children}
      </pre>
      {!multiline ? copyButton : null}
      <span role="status" className="sr-only">{copied ? t('skills.detail.codeCopied') : ''}</span>
    </div>
  );
}

function DocumentTable({ node: _node, children, ...props }: ComponentProps<'table'> & ExtraProps) {
  const { t } = useTranslation();
  const { ref, overflowing } = useHorizontalOverflow<HTMLDivElement>(children);
  return (
    <div ref={ref} className="skill-table-scroll" tabIndex={overflowing ? 0 : -1}
      role={overflowing ? 'region' : undefined} aria-label={overflowing ? t('skills.detail.documentTable') : undefined}>
      <table {...props}>{children}</table>
    </div>
  );
}

const components: Components = { pre: CodeBlock, table: DocumentTable, a: DocumentLink, img: DocumentImage };

export const DetailMarkdown = memo(function DetailMarkdown({ content }: { content: string }) {
  const prefix = useId();
  const rehypePlugins = useMemo<NonNullable<ComponentProps<typeof ReactMarkdown>['rehypePlugins']>>(
    () => [[rehypeDetailDocument, { prefix }]], [prefix]);
  return <div className="skill-markdown" tabIndex={-1}>
    <ReactMarkdown remarkPlugins={[remarkGfm, remarkDetailTargets]} rehypePlugins={rehypePlugins} components={components}>{content}</ReactMarkdown>
  </div>;
});
