import { createElement, useCallback, useContext, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type ReactNode, type ElementType } from 'react';
import { useTranslation } from 'react-i18next';
import { Languages, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { Dialog, DialogContent, DialogTitle, DialogDescription, DialogFooter } from '@/components/ui/dialog';
import { beginTranslation, cancelTranslation, translateUnits } from '@/hooks/useTauriApi';
import { useTranslationSettings, translationDraft, translationPreferences, rememberTranslation } from '@/stores/translation';
import { prepareElement, restoreElement, type PreparedElement } from '@/lib/translation/document';
import { DocumentLink } from './DetailResources';
import type { TranslationResult, TranslationUnit } from '@/bindings';
import './SkillDetail.css';
import { useNavigate } from 'react-router-dom';
import { captureReading, restoreReading, saveReadingReturn, getReadingReturn, clearReadingReturn } from '@/lib/translation/navigation';
import type { DiscoverSkillSummary } from '@/lib/discover/types';
import { TranslationContext, TranslationLayoutContext, type ReadingTranslation } from '@/hooks/useDetailTranslation';


export function DetailTranslationProvider({ documentKey, context, children, returnPath, discoverSkill }: {
  documentKey: string; context: string; children: ReactNode; returnPath?: string; discoverSkill?: DiscoverSkillSummary;
}) {
  const root = useRef<HTMLDivElement>(null);
  const pendingScroll = useRef<ReturnType<typeof captureReading> | null>(null);
  const preserve = useCallback(() => {
    if (root.current) pendingScroll.current = captureReading(root.current);
    requestAnimationFrame(() => {
      if (root.current && pendingScroll.current) { restoreReading(root.current, pendingScroll.current); pendingScroll.current = null; }
    });
  }, []);
  const restoreAfterLayout = useCallback(() => {
    if (root.current && pendingScroll.current) { restoreReading(root.current, pendingScroll.current); pendingScroll.current = null; }
  }, []);
  const layout = useMemo(() => ({ documentKey, preserve,
    expanded: getReadingReturn()?.documentKey === documentKey && Boolean(getReadingReturn()?.descriptionExpanded) }), [documentKey, preserve]);
  const settings = useTranslationSettings(s => s.settings);
  const settingsError = useTranslationSettings(s => s.error);
  const load = useTranslationSettings(s => s.load);
  const save = useTranslationSettings(s => s.save);
  const [manual, setManual] = useState<{ key: string; enabled: boolean } | null>(null);
  const enabled = (manual?.key === documentKey ? manual.enabled : translationPreferences.get(documentKey)) ?? settings?.autoTranslate ?? false;
  const [units, setUnits] = useState<Map<string, PreparedElement>>(new Map());
  const [operation, setOperation] = useState<{ documentKey: string; serviceKey: string; results: Map<string, TranslationResult>; busy: boolean; error: string | null } | null>(null);
  const [discardReturned, setDiscardReturned] = useState<string | null>(null);
  const returned = useMemo(() => getReadingReturn()?.documentKey === documentKey
    ? getReadingReturn()?.translation : undefined, [documentKey]);
  const [attempt, setAttempt] = useState(0);
  const operationRef = useRef(operation);
  useLayoutEffect(() => { operationRef.current = operation; }, [operation]);
  const retryFailed = useRef(false);
  const service = settings?.service;
  const serviceKey = settings ? settings.serviceRevision ?? JSON.stringify([service, service?.kind === 'openAiCompatible'
    ? settings.profiles.find(profile => profile.id === service.profileId) : null, settings.noticeAccepted]) : '';
  const current = operation?.documentKey === documentKey ? operation : null;
  const restoredResults = useMemo(() => {
    const restored = new Map<string, TranslationResult>();
    if (!returned || discardReturned === documentKey) return restored;
    const bySignature = new Map(returned.results.map(item => [item.signature, item.result]));
    for (const [id, unit] of units) {
      const result = bySignature.get(JSON.stringify([unit.text, unit.protectedTokens]));
      if (result) restored.set(id, { ...result, id });
    }
    return restored;
  }, [returned, discardReturned, documentKey, units]);
  const previousService = current?.serviceKey ?? (discardReturned !== documentKey ? returned?.serviceKey : undefined);
  const outdated = Boolean(previousService && previousService !== serviceKey);
  const results = current?.results ?? restoredResults;
  const notice = Boolean(enabled && settings && !settings.noticeAccepted);
  const register = useCallback((id: string, value: PreparedElement) => {
    setUnits(previous => new Map(previous).set(id, value));
    return () => { setUnits(previous => { const next = new Map(previous); next.delete(id); return next; }); };
  }, []);
  useEffect(() => { void load(); }, [load]);
  useEffect(() => {
    if (!enabled || !settings?.noticeAccepted || !units.size || outdated) return;
    let cancelled = false;
    let session: string | null = null;
    const existing = retryFailed.current ? operationRef.current?.results ?? restoredResults : restoredResults;
    retryFailed.current = false;
    const selected = [...units].filter(([id, unit]) => {
      const prose = unit.protectedTokens.reduce((text, token) => text.replaceAll(token, ''), unit.text);
      return existing.get(id)?.status !== 'translated' && /\p{L}/u.test(prose.replace(/\p{Script=Han}/gu, ''));
    });
    if (!selected.length) return;
    void (async () => {
      session = await beginTranslation(settings.revision);
      if (cancelled) { await cancelTranslation(session); return; }
      preserve();
      setOperation({ documentKey, serviceKey, results: new Map(existing), busy: true, error: null });
      for (let offset = 0; offset < selected.length; offset += 8) {
        if (cancelled) break;
        const batch: TranslationUnit[] = selected.slice(offset, offset + 8).map(([id, unit]) => ({ id, text: unit.text,
          protectedTokens: unit.protectedTokens, context: context.slice(0, 1500) }));
        const translated = await translateUnits(session, batch);
        if (cancelled) break;
        preserve();
        setOperation(previous => {
          if (!previous || previous.documentKey !== documentKey) return previous;
          const next = new Map(previous.results);
          for (const value of translated) {
            const source = units.get(value.id);
            next.set(value.id, value.status === 'translated' && source && !restoreElement(source, value.text)
              ? { id: value.id, status: 'failed', code: 'invalidSegments' } : value);
          }
          return { ...previous, results: next };
        });
      }
    })().catch(failure => {
      if (!cancelled) setOperation(previous => ({ documentKey, serviceKey,
        results: previous?.documentKey === documentKey ? previous.results : new Map(), busy: false,
        error: typeof failure === 'string' ? failure : 'network' }));
    }).finally(() => {
      if (session) void cancelTranslation(session).catch(() => {});
      if (!cancelled) setOperation(previous => previous ? { ...previous, busy: false } : previous);
    });
    return () => { cancelled = true; if (session) void cancelTranslation(session).catch(() => {}); };
  }, [enabled, settings, units, attempt, context, documentKey, serviceKey, outdated, preserve, restoredResults]);
  useLayoutEffect(restoreAfterLayout, [operation, enabled, restoreAfterLayout]);
  useEffect(() => {
    const snapshot = getReadingReturn();
    if (!snapshot || snapshot.documentKey !== documentKey || !root.current) return;
    const frame = requestAnimationFrame(() => {
      if (root.current) { restoreReading(root.current, snapshot); clearReadingReturn(); }
    });
    return () => cancelAnimationFrame(frame);
  }, [documentKey, units]);
  const toggle = () => {
    preserve();
    const next = !enabled;
    if (next && outdated) { setOperation(null); setDiscardReturned(documentKey); }
    setManual({ key: documentKey, enabled: next }); rememberTranslation(documentKey, next);
  };
  const retry = () => {
    retryFailed.current = !outdated;
    if (outdated) { setOperation(null); setDiscardReturned(documentKey); }
    setAttempt(value => value + 1);
  };
  const rememberReading = useCallback(() => {
    if (root.current) saveReadingReturn({ route: returnPath ?? window.location.pathname + window.location.search, documentKey,
      ...captureReading(root.current), discoverSkill,
      translation: { serviceKey: previousService ?? serviceKey, results: [...results].flatMap(([id, result]) => {
        const unit = units.get(id);
        return unit ? [{ signature: JSON.stringify([unit.text, unit.protectedTokens]), result }] : [];
      }) } });
  }, [returnPath, documentKey, discoverSkill, previousService, serviceKey, results, units]);
  useEffect(() => {
    const captureSettingsNavigation = (event: MouseEvent) => {
      if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
      const anchor = event.target instanceof Element ? event.target.closest<HTMLAnchorElement>('a[href]') : null;
      if (!anchor) return;
      const target = new URL(anchor.href, window.location.href);
      const currentUrl = new URL(window.location.href);
      if (target.protocol === currentUrl.protocol && target.host === currentUrl.host && target.pathname === '/settings') rememberReading();
    };
    document.addEventListener('click', captureSettingsNavigation, true);
    return () => document.removeEventListener('click', captureSettingsNavigation, true);
  }, [rememberReading]);
  const value: ReadingTranslation = { enabled, busy: Boolean(current?.busy && !outdated), outdated,
    error: current?.error ?? (settingsError ? 'invalidSettings' : null),
    results, register, toggle, retry, total: units.size, completed: [...results.values()].filter(r => r.status === 'translated').length,
    rememberReading };
  const { t } = useTranslation();
  return <TranslationContext.Provider value={value}>
    <TranslationLayoutContext.Provider value={layout}>
      <div ref={root} className="contents">{children}</div>
    </TranslationLayoutContext.Provider>
    <Dialog open={notice} onOpenChange={open => {
      if (!open) { setManual({ key: documentKey, enabled: false }); rememberTranslation(documentKey, false); }
    }}>
      <DialogContent><DialogTitle>{t('translation.noticeTitle')}</DialogTitle>
        <DialogDescription>{t('translation.notice')}</DialogDescription>
        <DialogFooter><Button onClick={() => {
          if (!settings) return;
          void save({ ...translationDraft(settings), noticeAccepted: true }).catch(() => {
            setManual({ key: documentKey, enabled: false });
            setOperation({ documentKey, serviceKey, results: new Map(), busy: false, error: 'invalidSettings' });
          });
        }}>{t('translation.start')}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  </TranslationContext.Provider>;
}

export function TranslationActions() {
  const translation = useContext(TranslationContext);
  const { t } = useTranslation();
  if (!translation) return null;
  return <Tooltip>
    <TooltipTrigger asChild>
      <Button variant="ghost" size="icon" aria-label={t('translation.action')} aria-pressed={translation.enabled}
        onClick={translation.toggle}
        className="skill-translation-toggle size-7 shrink-0 text-muted-foreground hover:text-foreground aria-pressed:text-primary">
        <Languages className="size-4" aria-hidden="true" />
      </Button>
    </TooltipTrigger>
    <TooltipContent>{t(translation.enabled ? 'translation.hide' : 'translation.show')}</TooltipContent>
  </Tooltip>;
}

function TranslationSettingsLink({ onOpen }: { onOpen: () => void }) {
  const navigate = useNavigate();
  const { t } = useTranslation();
  return <Button variant="link" size="sm" className="h-auto shrink-0 px-0 py-1 text-xs"
    onClick={() => { onOpen(); navigate('/settings?section=translation'); }}>{t('translation.settings')}</Button>;
}

export function TranslationStatus() {
  const translation = useContext(TranslationContext);
  const { t } = useTranslation();
  if (!translation?.enabled) return null;
  const failed = [...translation.results.values()].filter(r => r.status === 'failed').length;
  const firstFailure = [...translation.results.values()].find(r => r.status === 'failed');
  const errorCode = translation.error ?? (firstFailure?.status === 'failed' ? firstFailure.code : null);
  const needsSettings = Boolean(errorCode && ['invalidSettings', 'staleSettings', 'credentialUnavailable', 'authentication'].includes(errorCode));
  if (!translation.busy && !failed && !translation.error && !translation.outdated) return null;
  return <div className="flex flex-wrap items-center gap-x-3 gap-y-1 py-2 text-xs" role="status">
    {translation.busy ? <><RefreshCw className="size-3 animate-spin motion-reduce:animate-none" />{t('translation.progress', { count: translation.completed, total: translation.total })}</> :
      <><span>{translation.outdated ? t('translation.settingsChanged') : translation.error ? t(`translation.errors.${translation.error}`, { defaultValue: t('translation.failed') }) : `${t('translation.partialFailed', { count: failed })}${firstFailure?.status === 'failed' ? ` · ${t(`translation.errors.${firstFailure.code}`)}` : ''}`}</span>
        {needsSettings && !translation.outdated ? <TranslationSettingsLink onOpen={translation.rememberReading} /> :
          <Button variant="link" size="sm" className="h-auto shrink-0 px-0 py-1 text-xs" onClick={translation.retry}>{t(translation.outdated ? 'translation.retranslate' : 'translation.retry')}</Button>}</>}
  </div>;
}

function restoredReact(node: Node, key: number): ReactNode {
  if (node.nodeType === Node.TEXT_NODE) return node.textContent;
  if (!(node instanceof Element)) return null;
  const children = [...node.childNodes].map(restoredReact);
  if (node.tagName === 'A') return <DocumentLink key={key} href={node.getAttribute('href') ?? ''}>{children}</DocumentLink>;
  if (node.tagName === 'IMG') return null;
  const tag = node.tagName.toLowerCase();
  return createElement(tag, { key }, ...children);
}

export function TranslatedBlock({ as: Tag = 'span', children, className, ...props }: {
  as?: ElementType; children?: ReactNode; className?: string; [key: string]: unknown;
}) {
  const translation = useContext(TranslationContext);
  const id = useId();
  const ref = useRef<HTMLElement>(null);
  const [prepared, setPrepared] = useState<PreparedElement | null>(null);
  const register = translation?.register;
  useLayoutEffect(() => {
    if (!ref.current || !register) return;
    const copy = ref.current.cloneNode(true) as HTMLElement;
    copy.querySelectorAll('[data-translation-output]').forEach(node => node.remove());
    const value = prepareElement(copy);
    const signature = (item: PreparedElement) => JSON.stringify([item.text,
      [...item.fragments].map(([token, fragment]) => [token, fragment.element.outerHTML, fragment.closing, fragment.atomic])]);
    setPrepared(previous => previous && signature(previous) === signature(value) ? previous : value);
  }, [children, register]);
  useLayoutEffect(() => {
    if (prepared && register) return register(id, prepared);
  }, [prepared, id, register]);
  const result = translation?.results.get(id);
  const translated = useMemo(() => {
    if (!prepared || result?.status !== 'translated' || result.text.trim() === prepared.text.trim()) return null;
    const restored = restoreElement(prepared, result.text);
    return restored ? [...restored.childNodes].map(restoredReact) : null;
  }, [prepared, result]);
  return <Tag {...props} ref={ref} data-translation-source="" className={className}>
    {children}
    {translation?.enabled && translated ? <span data-translation-output="" lang="zh-CN" className="skill-translation">{translated}</span> : null}
  </Tag>;
}
