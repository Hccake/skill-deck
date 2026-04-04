import { useEffect, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import {
  X,
  Star,
  ShieldCheck,
  ShieldAlert,
  Github,
  Terminal,
  Activity,
  ChevronDown,
  ChevronRight,
  DownloadCloud,
  CheckCircle2,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Separator } from '@/components/ui/separator';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { getDiscoverSkillDetail } from '@/lib/discover/api';
import { formatInstalls } from '@/lib/discover/format';
import type { DiscoverSkillDetail } from '@/lib/discover/api';
import type { DiscoverAuditRisk, DiscoverSecurityAudit, DiscoverSkillSummary } from '@/lib/discover/types';

const PROSE_WITH_LISTS_CLASS_NAME = 'skill-prose skill-prose-with-lists';

interface DiscoverDetailPanelProps {
  skill: DiscoverSkillSummary;
  isInstalled: boolean;
  onClose: () => void;
  onInstall: (skill: DiscoverSkillSummary) => void;
}

const MIN_DETAIL_LOADING_MS = 180;

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function RiskBadge({ risk, t }: { risk?: DiscoverAuditRisk; t: (key: string) => string }) {
  const riskKey = risk ?? 'unknown';

  if (riskKey === 'unknown') {
    return null;
  }

  if (riskKey === 'safe' || riskKey === 'low') {
    return (
      <Badge
        variant="secondary"
        className="flex items-center gap-1.5 border-emerald-500/20 bg-emerald-500/12 text-emerald-700 hover:bg-emerald-500/18"
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
      statusClassName: 'text-emerald-700',
    };
  }

  if (status === 'warn') {
    return {
      dotClassName: 'bg-amber-500',
      statusClassName: 'text-amber-700',
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
    repoUrl: skill.source.startsWith('http') ? skill.source : `https://github.com/${skill.source}`,
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

export function DiscoverDetailPanel({
  skill,
  isInstalled,
  onClose,
  onInstall,
}: DiscoverDetailPanelProps) {
  const { t } = useTranslation();
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
  }, [skill.detailUrl]);

  const displayData = detail ?? createFallbackDetail(skill);
  const hasOverviewContent = Boolean(displayData.summaryHtml || displayData.description || displayData.highlights.length > 0);

  return (
    <div className="relative flex h-full flex-col overflow-hidden bg-surface">
      <div className="shrink-0 border-b bg-surface px-4 py-5 @sm:px-6 @md:px-8 @lg:px-10 @xl:px-12">
        <div className="flex items-center justify-between gap-4">
          <div className="min-w-0 flex-1 flex-col gap-2.5">
            <div className="flex min-w-0 flex-wrap items-center gap-2">
              <h2 className="truncate text-xl font-heading font-semibold leading-tight text-foreground">{displayData.name}</h2>
              <RiskBadge risk={displayData.auditRisk} t={t} />
            </div>

            <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-xs">
            {displayData.repoUrl ? (
              <a
                href={displayData.repoUrl}
                target="_blank"
                rel="noreferrer"
                className="inline-flex min-w-0 items-center gap-1.5 text-muted-foreground hover:text-foreground hover:underline"
              >
                <Github className="h-3.5 w-3.5 shrink-0" />
                <span className="truncate">{displayData.source}</span>
              </a>
            ) : (
              <div className="inline-flex min-w-0 items-center gap-1.5 text-muted-foreground">
                <Github className="h-3.5 w-3.5 shrink-0" />
                <span className="truncate">{displayData.source}</span>
              </div>
            )}

            {displayData.weeklyInstalls !== undefined && (
              <>
                <span className="h-1 w-1 rounded-full bg-border/80" />
                <HeaderMetaItem
                  icon={<Activity className="h-3.5 w-3.5" />}
                  label={t('skills.discover.weeklyInstalls')}
                  value={formatInstalls(displayData.weeklyInstalls)}
                />
              </>
            )}

            {displayData.stars !== undefined && (
              <>
                <span className="h-1 w-1 rounded-full bg-border/80" />
                <HeaderMetaItem
                  icon={<Star className="h-3.5 w-3.5 text-amber-500" />}
                  label={t('skills.discover.starsLabel')}
                  value={formatInstalls(displayData.stars)}
                />
              </>
            )}
          </div>
        </div>
        
        <div className="flex shrink-0 items-center gap-2">
            <Button
              className="h-8 shrink-0 rounded-full px-4 text-[13px] font-medium tracking-wide shadow-sm transition-transform active:scale-95"
              variant={isInstalled ? 'secondary' : 'default'}
              disabled={isInstalled}
              onClick={() => {
                if (!isInstalled) onInstall(skill);
              }}
            >
              {isInstalled ? (
                <>
                  <CheckCircle2 className="h-3.5 w-3.5" />
                  {t('skills.discover.installed')}
                </>
              ) : (
                <>
                  <DownloadCloud className="h-3.5 w-3.5" />
                  {t('skills.discover.install')}
                </>
              )}
            </Button>
            <Button variant="ghost" size="icon" onClick={onClose} className="h-8 w-8 text-muted-foreground hover:bg-accent hover:text-foreground">
              <X className="h-4 w-4" />
            </Button>
          </div>
        </div>
      </div>

      <div className="min-h-0 flex-1 bg-surface @container">
        <ScrollArea className="h-full">
          <div className="space-y-6 px-4 py-5 @sm:px-6 @md:px-8 @lg:px-10 @xl:px-12 @lg:py-6">
            {loading ? (
              <div className="space-y-5" data-testid="discover-detail-skeleton">
                <Skeleton className="h-5 w-52 rounded-md" />
                <div className="flex flex-wrap items-start gap-8">
                  <div className="flex-1 min-w-[320px] max-w-3xl space-y-5">
                    <Skeleton className="h-28 w-full rounded-lg" />
                    <Skeleton className="h-64 w-full rounded-lg" />
                    <Skeleton className="h-10 w-44 rounded-lg" />
                  </div>
                  <aside className="w-full md:w-[280px] shrink-0 space-y-4 pt-1">
                    <Skeleton className="h-10 w-full rounded-lg" />
                    <Skeleton className="h-24 w-full rounded-lg" />
                    <Skeleton className="h-20 w-full rounded-lg" />
                  </aside>
                </div>
              </div>
            ) : error ? (
              <div className="flex flex-col items-center justify-center py-16 text-center text-muted-foreground">
                <ShieldAlert className="mb-3 h-8 w-8 text-destructive" />
                <p className="text-sm">{t('skills.discover.loadDetailErrorTitle')}</p>
                <p className="mt-1 text-xs">{t('skills.discover.loadDetailErrorHint')}</p>
              </div>
            ) : (
              <div className="flex flex-wrap items-start gap-10">
                <main className="min-w-[320px] max-w-3xl flex-1 space-y-8">
                  {hasOverviewContent && (
                    <section className="space-y-3">
                      <div className="text-[11px] font-heading font-semibold uppercase tracking-[0.16em] text-muted-foreground">
                        {t('skills.discover.overview')}
                      </div>

                      {displayData.summaryHtml ? (
                        <div className={PROSE_WITH_LISTS_CLASS_NAME} dangerouslySetInnerHTML={{ __html: displayData.summaryHtml }} />
                      ) : displayData.description ? (
                        <div className="space-y-3">
                          <p className="text-sm leading-7 text-foreground/82">{displayData.description}</p>
                          {displayData.highlights.length > 0 && (
                            <ul className="list-disc space-y-2 pl-5 text-sm leading-7 text-foreground/82 marker:text-primary/70">
                              {displayData.highlights.map((item, idx) => (
                                <li key={idx}>{item}</li>
                              ))}
                            </ul>
                          )}
                        </div>
                      ) : (
                        <ul className="list-disc space-y-2 pl-5 text-sm leading-7 text-foreground/82 marker:text-primary/70">
                          {displayData.highlights.map((item, idx) => (
                            <li key={idx}>{item}</li>
                          ))}
                        </ul>
                      )}
                    </section>
                  )}

                  {displayData.contentHtml && (
                    <>
                      {hasOverviewContent && <Separator className="bg-border/60" />}

                      <section className="space-y-3">
                        <div className="text-[11px] font-heading font-semibold uppercase tracking-[0.16em] text-muted-foreground">
                          {t('skills.discover.skillContent')}
                        </div>
                        <div className={PROSE_WITH_LISTS_CLASS_NAME} dangerouslySetInnerHTML={{ __html: displayData.contentHtml }} />
                      </section>
                    </>
                  )}

                  {displayData.installCommand && (
                    <>
                      <Separator className="bg-border/60" />

                      <Collapsible open={installCommandOpen} onOpenChange={setInstallCommandOpen}>
                        <section className="space-y-2">
                          <CollapsibleTrigger asChild>
                            <button
                              type="button"
                              className="flex w-full items-center justify-between gap-3 rounded-md px-2 py-1.5 text-left hover:bg-accent/25"
                            >
                              <div className="flex items-center gap-2">
                                <Terminal className="h-4 w-4 text-muted-foreground" />
                                <span className="text-[11px] font-heading font-semibold uppercase tracking-[0.16em] text-muted-foreground">
                                  {t('skills.discover.installViaCli')}
                                </span>
                              </div>

                              {installCommandOpen ? (
                                <ChevronDown className="h-4 w-4 text-muted-foreground" />
                              ) : (
                                <ChevronRight className="h-4 w-4 text-muted-foreground" />
                              )}
                            </button>
                          </CollapsibleTrigger>

                          <CollapsibleContent className="pt-1">
                            <code className="block w-full overflow-x-auto break-all rounded-md border border-border/60 bg-accent/20 p-3 text-xs leading-relaxed text-muted-foreground">
                              {displayData.installCommand}
                            </code>
                          </CollapsibleContent>
                        </section>
                      </Collapsible>
                    </>
                  )}
                </main>

                <aside className="w-full shrink-0 space-y-6 md:w-[280px] pt-1">
                  {displayData.firstSeen && (
                    <section className="space-y-1.5">
                      <div className="text-[11px] font-heading font-semibold uppercase tracking-[0.16em] text-muted-foreground">
                        {t('skills.discover.firstSeen')}
                      </div>
                      <div className="text-sm text-foreground/88">{displayData.firstSeen}</div>
                    </section>
                  )}

                  {displayData.securityAudits.length > 0 && (
                    <section className="space-y-2.5">
                      <div className="text-[11px] font-heading font-semibold uppercase tracking-[0.16em] text-muted-foreground">
                        {t('skills.discover.securityAudits')}
                      </div>

                      <div className="space-y-1.5">
                        {displayData.securityAudits.map((audit) => {
                          const tone = getAuditTone(audit.status);

                          return (
                            <a
                              key={audit.url}
                              href={audit.url}
                              target="_blank"
                              rel="noreferrer"
                              data-status={audit.status}
                              className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 rounded-md py-1.5 text-sm hover:text-foreground"
                            >
                              <div className="min-w-0 flex items-center gap-2">
                                <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${tone.dotClassName}`} />
                                <span className="truncate text-foreground/88">{audit.name}</span>
                              </div>
                              <span className={`text-[11px] font-semibold uppercase tracking-[0.12em] ${tone.statusClassName}`}>
                                {t(`skills.discover.securityStatus.${audit.status}`)}
                              </span>
                            </a>
                          );
                        })}
                      </div>
                    </section>
                  )}

                  {displayData.installedOn.length > 0 && (
                    <section className="space-y-2.5">
                      <div className="text-[11px] font-heading font-semibold uppercase tracking-[0.16em] text-muted-foreground">
                        {t('skills.discover.installedOn')}
                      </div>

                      <div className="space-y-1.5">
                        {displayData.installedOn.map((entry) => (
                          <div
                            key={`${entry.agent}-${entry.installsText}`}
                            className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 py-1.5 text-sm"
                          >
                            <span className="truncate text-foreground/88">{entry.agent}</span>
                            <span className="whitespace-nowrap font-mono text-xs text-muted-foreground">{entry.installsText}</span>
                          </div>
                        ))}
                      </div>
                    </section>
                  )}
                </aside>
              </div>
            )}
          </div>
        </ScrollArea>
      </div>


    </div>
  );
}