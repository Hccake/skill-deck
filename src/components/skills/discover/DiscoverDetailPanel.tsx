import { DetailTranslationProvider, TranslationActions, TranslationStatus, TranslatedBlock } from '../detail/DetailTranslation';
import { useEffect, useRef, useState, type MouseEvent, type ReactNode } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { toast } from 'sonner';
import { isOpenableUrl } from '@/lib/skill-card-presentation';
import { useTranslation } from 'react-i18next';
import {
  X,
  Star,
  ShieldCheck,
  ShieldAlert,
  Link2,
  Terminal,
  ChevronDown,
  ChevronRight,
  DownloadCloud,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { OverflowTooltip } from '@/components/ui/overflow-tooltip';
import { DiscoverRichText } from './DiscoverRichText';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { getDiscoverSkillDetail } from '@/lib/discover/api';
import { formatInstalls } from '@/lib/discover/format';
import type { DiscoverSkillDetail } from '@/lib/discover/api';
import type { DiscoverAuditRisk, DiscoverSecurityAudit, DiscoverSkillSummary } from '@/lib/discover/types';
import { delay } from '@/lib/discover-utils';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import { projectPathBasename } from '@/lib/projects/presentation';

interface DiscoverDetailPanelProps {
  skill: DiscoverSkillSummary;
  installLocations: string[];
  onClose: () => void;
  onInstall: (skill: DiscoverSkillSummary) => void;
}

const MIN_DETAIL_LOADING_MS = 180;

function RiskBadge({ risk, t }: { risk?: DiscoverAuditRisk; t: (key: string) => string }) {
  const riskKey = risk ?? 'unknown';

  if (riskKey === 'unknown') {
    return null;
  }

  if (riskKey === 'safe' || riskKey === 'low') {
    return (
      <Badge
        variant="secondary"
        className="flex items-center gap-1.5 border-emerald-500/20 bg-emerald-500/12 text-emerald-700 dark:text-emerald-400"
      >
        <ShieldCheck className="h-3 w-3" />
        {t(`skills.discover.riskBadge.${riskKey}`)}
      </Badge>
    );
  }

  return (
    <Badge variant="destructive" className="flex items-center gap-1.5">
      <ShieldAlert className="h-3 w-3" />
      {t(`skills.discover.riskBadge.${riskKey}`)}
    </Badge>
  );
}

function getAuditTone(status: DiscoverSecurityAudit['status']) {
  if (status === 'pass') {
    return {
      dotClassName: 'bg-emerald-500',
      statusClassName: 'text-emerald-700 dark:text-emerald-400',
    };
  }

  if (status === 'warn') {
    return {
      dotClassName: 'bg-amber-500',
      statusClassName: 'text-amber-700 dark:text-amber-400',
    };
  }

  if (status === 'fail') {
    return {
      dotClassName: 'bg-destructive',
      statusClassName: 'text-destructive',
    };
  }

  return {
    dotClassName: 'bg-border',
    statusClassName: 'text-muted-foreground',
  };
}

function createFallbackDetail(skill: DiscoverSkillSummary): DiscoverSkillDetail {
  return {
    ...skill,
    description: skill.summary,
    summaryHtml: undefined,
    highlights: [],
    repoUrl: undefined,
    installCommand: undefined,
    firstSeen: undefined,
    securityAudits: [],
    installedOn: [],
    contentHtml: undefined,
  };
}

function HeaderMetaItem({ icon, label, value }: { icon: ReactNode; label: string; value: string }) {
  return (
    <div className="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
      {icon}
      <span>{label}</span>
      <span className="font-medium text-foreground/90">{value}</span>
    </div>
  );
}

function SectionTitle({ children }: { children: ReactNode }) {
  return <h3 className="mb-2 text-xs font-medium leading-[18px] text-muted-foreground">{children}</h3>;
}

export function DiscoverDetailPanel({
  skill,
  installLocations,
  onClose,
  onInstall,
}: DiscoverDetailPanelProps) {
  const { t } = useTranslation();
  const writeBlocked = useBusinessWriteBlocked();
  const headingRef = useRef<HTMLHeadingElement>(null);
  const [retryCount, setRetryCount] = useState(0);
  const [detail, setDetail] = useState<DiscoverSkillDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const [installCommandOpen, setInstallCommandOpen] = useState(false);

  useEffect(() => {
    let cancelled = false;

    setLoading(true);
    setError(false);
    setDetail(null);
    setInstallCommandOpen(false);

    const loadDetail = async () => {
      const startedAt = Date.now();

      try {
        const data = await getDiscoverSkillDetail(skill.detailUrl);
        const remaining = MIN_DETAIL_LOADING_MS - (Date.now() - startedAt);
        if (remaining > 0) {
          await delay(remaining);
        }

        if (cancelled) return;
        setDetail(data);
      } catch (err) {
        console.error('Failed to load skill detail:', err);
        if (cancelled) return;
        setError(true);
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    };

    void loadDetail();

    return () => {
      cancelled = true;
    };
  }, [skill.detailUrl, retryCount]);

  const handleLink = (event: MouseEvent<HTMLElement>) => {
    if (event.defaultPrevented || (event.type === 'auxclick' && event.button !== 1)) return;
    const anchor = event.target instanceof Element ? event.target.closest('a[href]') : null;
    if (!anchor || !event.currentTarget.contains(anchor)) return;
    event.preventDefault();
    const target = anchor.getAttribute('href');
    if (!isOpenableUrl(target)) return;
    void openUrl(target).catch(() => toast.error(t('skills.detail.linkOpenFailed')));
  };

  const displayData = detail ?? createFallbackDetail(skill);
  const hasOverviewContent = Boolean(displayData.summaryHtml || displayData.description || displayData.highlights.length > 0);

  const hasRisk = Boolean(displayData.auditRisk && displayData.auditRisk !== 'unknown');
  const hasDecisionInfo = installLocations.length > 0 || displayData.securityAudits.length > 0 || hasRisk;
  const hasExtraInfo = displayData.weeklyInstalls !== undefined || displayData.installedOn.length > 0 || Boolean(displayData.firstSeen);

  return (
    <DetailTranslationProvider discoverSkill={skill} key={skill.detailUrl} documentKey={JSON.stringify([skill.detailUrl, displayData.name, displayData.description, displayData.summaryHtml, displayData.highlights, displayData.contentHtml])} context={`${displayData.name} ${displayData.description ?? ''}`}>
    <div className="discover-detail @container relative flex h-full flex-col overflow-hidden bg-surface" onClick={handleLink} onAuxClick={handleLink}>
      <div role="group" aria-label={t('skills.discover.currentSkill')} className="discover-detail-header shrink-0 border-b bg-surface">
        <div className="discover-header-identity min-w-0">
          <div className="discover-heading-row detail-title-grid">
          <TranslatedBlock as="div" className="detail-title-copy">
          <OverflowTooltip content={displayData.name}>
              <h2 ref={headingRef} tabIndex={0} className="min-w-0 flex-1 truncate rounded-sm font-heading text-xl font-semibold leading-tight text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring">
                {displayData.name}
              </h2>
          </OverflowTooltip>
          </TranslatedBlock>
          <TranslationActions />
          </div>
          <div className="discover-header-meta">
            <OverflowTooltip always={isOpenableUrl(displayData.repoUrl) && displayData.repoUrl !== displayData.source}
              content={<><p>{displayData.source}</p>{isOpenableUrl(displayData.repoUrl) && displayData.repoUrl !== displayData.source ? <p>{displayData.repoUrl}</p> : null}</>}>
                {isOpenableUrl(displayData.repoUrl) ? (
                  <a href={displayData.repoUrl} className="discover-source-link inline-flex min-w-0 max-w-full items-start gap-1.5 rounded-sm text-xs leading-[18px] text-muted-foreground hover:text-foreground">
                    <Link2 className="mt-0.5 size-3.5 shrink-0" /><span className="discover-source-name">{displayData.source}</span>
                  </a>
                ) : (
                  <span tabIndex={0} className="inline-flex min-w-0 max-w-full items-start gap-1.5 rounded-sm text-xs leading-[18px] text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring">
                    <Link2 className="mt-0.5 size-3.5 shrink-0" /><span className="discover-source-name">{displayData.source}</span>
                  </span>
                )}
            </OverflowTooltip>
            {displayData.stars !== undefined && <HeaderMetaItem icon={<Star className="size-3.5 text-amber-500" />} label={t('skills.discover.starsLabel')} value={formatInstalls(displayData.stars)} />}
          </div>
        </div>
        <div className="discover-header-actions flex shrink-0 flex-wrap items-center gap-2">
          <Button className="h-8 shrink-0 rounded-full px-4 text-[13px] font-medium shadow-sm" onClick={() => onInstall(skill)} disabled={writeBlocked}>
            <DownloadCloud className="h-3.5 w-3.5" />{t('skills.discover.install')}
          </Button>
          <Button variant="ghost" size="icon" onClick={onClose} aria-label={t('common.close')} className="h-8 w-8 text-muted-foreground hover:bg-accent hover:text-foreground">
            <X className="h-4 w-4" />
          </Button>
        </div>
      </div>
      <span role="status" className="sr-only">{loading ? t('skills.discover.loadingDetail') : ''}</span>
      <div className="min-h-0 flex-1">
        <ScrollArea className="h-full">
          <div className="discover-detail-content" aria-busy={loading}>
            {loading ? (
              <div className="space-y-4" data-testid="discover-detail-skeleton">
                <Skeleton className="h-5 w-1/2" /><Skeleton className="h-24 w-full" /><Skeleton className="h-48 w-full" />
              </div>
            ) : error ? (
              <div role="alert" className="flex flex-col items-center py-10 text-center">
                <ShieldAlert className="mb-3 size-7 text-destructive" />
                <p className="text-sm">{t('skills.discover.loadDetailErrorTitle')}</p>
                <p className="mt-1 text-xs text-muted-foreground">{t('skills.discover.loadDetailErrorHint')}</p>
                <Button variant="outline" size="sm" className="mt-3" onClick={() => {
                  headingRef.current?.focus({ preventScroll: true });
                  setRetryCount(count => count + 1);
                }}>{t('skills.discover.retry')}</Button>
              </div>
            ) : (
              <div className="discover-detail-grid" data-has-aside={hasDecisionInfo || hasExtraInfo} data-has-decision={hasDecisionInfo}>
                <div className="discover-reading min-w-0 space-y-6">
                  <TranslationStatus />
                  {hasOverviewContent && <section>
                    <SectionTitle>{t('skills.discover.overview')}</SectionTitle>
                    {displayData.summaryHtml ? <DiscoverRichText html={displayData.summaryHtml} /> : (
                      <div className="skill-prose space-y-3">
                        {displayData.description && <TranslatedBlock as="p">{displayData.description}</TranslatedBlock>}
                        {displayData.highlights.length > 0 && <ul className="list-disc space-y-1 pl-5">{displayData.highlights.map((item, index) => <TranslatedBlock as="li" key={index}>{item}</TranslatedBlock>)}</ul>}
                      </div>
                    )}
                  </section>}
                  {displayData.contentHtml && <section>
                    <SectionTitle>{t('skills.discover.skillContent')}</SectionTitle>
                    <DiscoverRichText html={displayData.contentHtml} />
                  </section>}
                  {displayData.installCommand && <Collapsible open={installCommandOpen} onOpenChange={setInstallCommandOpen}>
                    <CollapsibleTrigger asChild>
                      <button type="button" className="flex w-full items-center justify-between gap-3 rounded-md py-1.5 text-left outline-none hover:bg-accent/25 focus-visible:ring-2 focus-visible:ring-ring">
                        <span className="flex items-center gap-2 text-xs font-medium text-muted-foreground"><Terminal className="size-4" />{t('skills.discover.installViaCli')}</span>
                        {installCommandOpen ? <ChevronDown className="size-4 text-muted-foreground" /> : <ChevronRight className="size-4 text-muted-foreground" />}
                      </button>
                    </CollapsibleTrigger>
                    <CollapsibleContent className="pt-2">
                      <code className="block w-full rounded-md border border-border bg-accent/20 p-3 text-xs leading-5 [overflow-wrap:anywhere]">{displayData.installCommand}</code>
                    </CollapsibleContent>
                  </Collapsible>}
                </div>
                {hasDecisionInfo && <div className="discover-decision-info min-w-0 space-y-6">
                  {installLocations.length > 0 && <section>
                    <SectionTitle>{t('skills.discover.installedWorkspaces')}</SectionTitle>
                    <div className="space-y-2">
                      {installLocations.map(location => location === 'global' ? (
                        <p key={location} className="text-[13px] leading-5">{t('context.global')}</p>
                      ) : (
                        <div key={location} className="min-w-0">
                          <p className="truncate text-[13px] leading-5">{projectPathBasename(location)}</p>
                          <Tooltip>
                            <TooltipTrigger asChild>
                              <code tabIndex={0} className="block truncate rounded-sm text-xs leading-[18px] text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring">{location}</code>
                            </TooltipTrigger>
                            <TooltipContent className="max-w-[min(32rem,90vw)] text-wrap break-all font-mono">{location}</TooltipContent>
                          </Tooltip>
                        </div>
                      ))}
                    </div>
                  </section>}
                  {(displayData.securityAudits.length > 0 || hasRisk) && <section>
                    <SectionTitle>{t('skills.discover.securityAudits')}</SectionTitle>
                    {hasRisk && <div className="mb-2 flex"><RiskBadge risk={displayData.auditRisk} t={t} /></div>}
                    <div className="space-y-1">
                      {displayData.securityAudits.map(audit => {
                        const tone = getAuditTone(audit.status);
                        const content = <>
                          <span className="flex min-w-0 items-center gap-2"><span aria-hidden="true" className={`size-1.5 shrink-0 rounded-full ${tone.dotClassName}`} /><span className="min-w-0 truncate">{audit.name}</span></span>
                          <span className={`text-xs font-medium ${tone.statusClassName}`}>{t(`skills.discover.securityStatus.${audit.status}`)}</span>
                        </>;
                        return <Tooltip key={audit.url}>
                          <TooltipTrigger asChild>
                            {isOpenableUrl(audit.url) ? (
                              <a href={audit.url} data-status={audit.status} className="discover-audit-link grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 rounded-sm py-1 text-[13px] leading-5">{content}</a>
                            ) : <div data-status={audit.status} className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 py-1 text-[13px] leading-5">{content}</div>}
                          </TooltipTrigger>
                          <TooltipContent className="max-w-[min(32rem,90vw)] text-wrap break-all"><p>{audit.name}</p><p>{audit.url}</p></TooltipContent>
                        </Tooltip>;
                      })}
                    </div>
                  </section>}
                </div>}
                {hasExtraInfo && <div className="discover-extra-info min-w-0 space-y-6">
                  {displayData.weeklyInstalls !== undefined && <section>
                    <SectionTitle>{t('skills.discover.weeklyInstalls')}</SectionTitle>
                    <p className="text-[13px] leading-5 tabular-nums">{formatInstalls(displayData.weeklyInstalls)}</p>
                  </section>}
                  {displayData.installedOn.length > 0 && <section>
                    <SectionTitle>{t('skills.discover.installedOn')}</SectionTitle>
                    <div className="space-y-1">
                      {displayData.installedOn.map(entry => <div key={`${entry.agent}-${entry.installsText}`} className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 py-1 text-[13px] leading-5">
                        <span>{entry.agent}</span><span className="text-xs tabular-nums text-muted-foreground">{entry.installsText}</span>
                      </div>)}
                    </div>
                  </section>}
                  {displayData.firstSeen && <section>
                    <SectionTitle>{t('skills.discover.firstSeen')}</SectionTitle>
                    <p className="text-[13px] leading-5">{displayData.firstSeen}</p>
                  </section>}
                </div>}
              </div>
            )}
          </div>
        </ScrollArea>
      </div>
    </div>
    </DetailTranslationProvider>
  );
}
