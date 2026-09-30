import { useEffect, useRef, useState, type ComponentProps, type MouseEvent, type ReactElement } from 'react';
import { useTranslation } from 'react-i18next';
import type { ExtraProps } from 'react-markdown';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Check, Copy, ImageOff, RefreshCw } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { isOpenableUrl } from '@/lib/skill-card-presentation';
import { originalTarget } from './detail-markdown-tree';

function ResourceHint({ target, reason, children }: { target: string; reason?: string; children: ReactElement }) {
  return <Tooltip><TooltipTrigger asChild>{children}</TooltipTrigger>
    <TooltipContent className="max-w-[min(32rem,calc(100vw-2rem))] break-all text-wrap">
      <div className="max-h-48 overflow-auto">
        {reason ? <p>{reason}</p> : null}
        <p>{target}</p>
      </div>
    </TooltipContent>
  </Tooltip>;
}

function AddressCopy({ target, label, reason }: { target: string; label: string; reason: string }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(timer);
  }, [copied]);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(target);
      setCopied(true);
    } catch {
      toast.error(t('skills.detail.copyAddressFailed'));
    }
  };
  return <>
    <ResourceHint target={target} reason={copied ? t('skills.detail.addressCopied') : reason}>
      <Button type="button" variant="ghost" size="icon" className="skill-resource-action size-6 text-muted-foreground hover:text-foreground"
        aria-label={label} onClick={() => void copy()}>
        {copied ? <Check className="size-3" aria-hidden="true" /> : <Copy className="size-3" aria-hidden="true" />}
      </Button>
    </ResourceHint>
    <span role="status" className="sr-only">{copied ? t('skills.detail.addressCopied') : ''}</span>
  </>;
}

export function DocumentLink({ node, href, title, target: explicitTarget, children, ...props }: Omit<ComponentProps<'a'>, 'target'> & ExtraProps & { target?: string }) {
  const { t } = useTranslation();
  const target = explicitTarget ?? originalTarget(node, href);
  const fragment = target.startsWith('#');
  if (!fragment && !isOpenableUrl(target)) {
    if (!target) return <span>{children}</span>;
    const reason = t('skills.detail.referenceUnsupported');
    return <span className="skill-document-reference">
      <ResourceHint target={target} reason={reason}><span>{children}</span></ResourceHint>
      <AddressCopy key={target} target={target} label={t('skills.detail.copyReferenceAddress')} reason={reason} />
    </span>;
  }

  const activate = (event: MouseEvent<HTMLAnchorElement>) => {
    if (event.type === 'auxclick' && event.button !== 1) return;
    event.preventDefault();
    if (!fragment) {
      void openUrl(target).catch(() => toast.error(t('skills.detail.linkOpenFailed')));
      return;
    }
    const root = event.currentTarget.closest<HTMLElement>('.skill-markdown');
    const viewport = root?.closest<HTMLElement>('[data-radix-scroll-area-viewport]');
    let anchor: string;
    try { anchor = decodeURIComponent(target.slice(1)); }
    catch { toast.error(t('skills.detail.chapterMissing')); return; }
    const heading = anchor
      ? [...root?.querySelectorAll<HTMLElement>('[data-document-anchor]') ?? []].find(element => element.dataset.documentAnchor === anchor)
      : root;
    if (!heading || !viewport) { toast.error(t('skills.detail.chapterMissing')); return; }
    viewport.scrollTo({ top: Math.max(0, viewport.scrollTop + heading.getBoundingClientRect().top - viewport.getBoundingClientRect().top - 8), behavior: 'instant' });
    heading.focus({ preventScroll: true });
  };

  return <ResourceHint target={target} reason={title}>
    <a {...props} href={target} className="skill-document-link" onClick={activate} onAuxClick={activate}>{children}</a>
  </ResourceHint>;
}

function ImageResource({ source, alt, title, linkTarget, linkTitle }: { source: string; alt: string; title?: string; linkTarget?: string; linkTitle?: string }) {
  const { t } = useTranslation();
  const supported = isOpenableUrl(source) && new URL(source).protocol === 'https:';
  const [phase, setPhase] = useState<'loading' | 'ready' | 'failed' | 'retrying'>('loading');
  const [attempt, setAttempt] = useState(0);
  const image = useRef<HTMLImageElement>(null);
  const retry = useRef<HTMLButtonElement>(null);
  const restoreFocus = useRef(false);
  useEffect(() => {
    if (phase === 'ready' && restoreFocus.current) {
      (image.current?.closest<HTMLElement>('a') ?? image.current)?.focus({ preventScroll: true });
      restoreFocus.current = false;
    }
  }, [phase]);

  const fallback = !supported || phase === 'failed' || phase === 'retrying';
  const reason = t(!supported ? 'skills.detail.imageUnsupported'
    : phase === 'retrying' ? 'skills.detail.imageLoading' : 'skills.detail.imageFailed');
  const label = alt || t('skills.detail.image');
  const visual = <>
    {supported ? <img key={attempt} ref={image} src={source} alt={alt} title={title} hidden={fallback} tabIndex={-1}
      onError={() => setPhase('failed')}
      onLoad={() => {
        restoreFocus.current = phase === 'retrying' && document.activeElement === retry.current;
        setPhase('ready');
      }} /> : null}
    {fallback ? <span className="skill-image-fallback">
      <ImageOff className="size-3.5 shrink-0" aria-hidden="true" />
      <span className="skill-image-message"><span>{label}</span><span className="text-muted-foreground">{reason}</span></span>
    </span> : null}
  </>;

  return <span className="skill-document-image">
    {linkTarget ? <DocumentLink target={linkTarget} title={linkTitle}>{visual}</DocumentLink> : visual}
    {fallback && supported ? <ResourceHint target={source} reason={`${reason} · ${label}`}>
      <Button ref={retry} type="button" variant="ghost" size="icon" className="skill-resource-action size-6 text-muted-foreground hover:text-foreground"
        aria-label={t('skills.detail.retryImage')} aria-disabled={phase === 'retrying'} onClick={() => {
          if (phase === 'retrying') return;
          setPhase('retrying');
          setAttempt(value => value + 1);
        }}>
        <RefreshCw className={phase === 'retrying' ? 'size-3 animate-spin motion-reduce:animate-none' : 'size-3'} aria-hidden="true" />
      </Button>
    </ResourceHint> : null}
    {!supported && source ? <AddressCopy target={source} label={t('skills.detail.copyImageAddress')} reason={`${reason} · ${label}`} /> : null}
  </span>;
}

export function DocumentImage({ node, src, alt = '', title }: ComponentProps<'img'> & ExtraProps) {
  const source = originalTarget(node, typeof src === 'string' ? src : '');
  return <ImageResource key={source} source={source} alt={alt} title={title} linkTarget={node?.data?.linkTarget} linkTitle={node?.data?.linkTitle} />;
}
