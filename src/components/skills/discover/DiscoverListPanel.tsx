import { useState, useEffect, useRef, useCallback, memo } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { Search, Download, AlertCircle, ShieldCheck, X } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { getDiscoverLeaderboard, searchDiscoverSkills } from '@/lib/discover/api';
import type { DiscoverSkillSummary, DiscoverTab } from '@/lib/discover/types';
import { useSkillListKeyboard } from '@/hooks/useSkillListKeyboard';
import { MIN_LOADING_MS, delay, isSkillInstalled } from '@/lib/discover-utils';

const LEADERBOARD_LOAD_DELAY_MS = 50;
const DISCOVER_TAB_LIST_STYLE = { background: 'transparent', boxShadow: 'none' } as const;
const DISCOVER_TAB_STYLE = { background: 'transparent', boxShadow: 'none', borderTop: 'none', borderLeft: 'none', borderRight: 'none' } as const;
const DISCOVER_TAB_CLASS = "px-0 pt-2 pb-1.5 relative text-[13px] font-medium text-muted-foreground bg-transparent rounded-none !border-x-0 !border-t-0 border-b-[2px] border-transparent mb-[-1px] transition-colors hover:text-foreground data-[state=active]:!bg-transparent data-[state=active]:!shadow-none data-[state=active]:text-foreground data-[state=active]:border-foreground data-[state=active]:font-semibold outline-none focus-visible:outline-2 focus-visible:outline-ring focus-visible:outline-offset-2";

function splitHotMetric(rawText: string): { primary: string; delta?: string } {
  const trimmed = rawText.trim();
  const match = trimmed.match(/^(.+?)\s+([+-]\S+)$/);

  if (!match) {
    return { primary: trimmed };
  }

  return {
    primary: match[1],
    delta: match[2],
  };
}

function renderDisplayMetric(metric: DiscoverSkillSummary['displayMetric']) {
  if (metric.kind !== 'hot') {
    return <span className="text-muted-foreground">{metric.rawText}</span>;
  }

  const { primary, delta } = splitHotMetric(metric.rawText);

  return (
    <>
      <span className="text-muted-foreground tabular-nums" data-testid="discover-hot-metric-value">
        {primary}
      </span>
      {delta ? (
        <span
          className="font-medium tabular-nums text-emerald-600 dark:text-emerald-400"
          data-testid="discover-hot-metric-delta"
        >
          {delta}
        </span>
      ) : null}
    </>
  );
}

interface DiscoverListPanelProps {
  installedSkillLocations: Map<string, string[]>;
  onSelect: (skill: DiscoverSkillSummary) => void;
  selectedDetailUrl?: string;
  activeTab: DiscoverTab;
  onTabChange: (tab: DiscoverTab) => void;
}

const DiscoverSkillItem = memo(function DiscoverSkillItem({
  skill,
  isInstalled,
  isSelected,
  onSelect,
  t,
}: {
  skill: DiscoverSkillSummary;
  isInstalled: boolean;
  isSelected: boolean;
  onSelect: (skill: DiscoverSkillSummary) => void;
  t: TFunction;
}) {
  return (
    <Tooltip>
    <TooltipTrigger asChild>
    <button
      type="button"
      data-skill-item={skill.detailUrl}
      data-skill-open
      aria-current={isSelected ? 'true' : undefined}
      className={`discover-item w-full px-3 py-2 text-left cursor-pointer transition-colors rounded-lg border border-transparent outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring ${
        isSelected
          ? 'bg-accent/60 border-accent/80'
          : 'hover:bg-accent/40 hover:border-accent/50'
      }`}
      data-testid="discover-skill-item"
      data-selected={isSelected ? 'true' : 'false'}
      onClick={() => onSelect(skill)}
    >
      <span className="discover-item-identity min-w-0">
        <span className="flex items-center gap-2">
          <span className="min-w-0 font-heading font-semibold text-sm leading-5 tracking-tight truncate">
            {skill.name}
          </span>
          {skill.isOfficial && (
            <ShieldCheck
              className="h-3 w-3 text-blue-500 shrink-0"
              aria-label={t('skills.discover.official')}
            />
          )}
        </span>
        <span className="block text-xs leading-[18px] text-muted-foreground truncate">
          {skill.source}
        </span>
      </span>
      <span className="discover-item-metrics flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs leading-4">
        {isInstalled && (
          <Badge variant="secondary" className="text-xs leading-4 hover:bg-secondary">
            {t('skills.discover.installed')}
          </Badge>
        )}
        {skill.displayMetric.rawText ? (
          <span className="inline-flex min-w-0 flex-wrap items-center gap-1.5 text-xs tabular-nums">
            <Download className="h-3 w-3 text-muted-foreground" />
            {renderDisplayMetric(skill.displayMetric)}
          </span>
        ) : null}
      </span>
    </button>
    </TooltipTrigger>
    <TooltipContent className="max-w-[min(32rem,90vw)] text-wrap break-all text-left data-[state=closed]:animate-none!">
      <p className="font-medium">{skill.name}</p>
      <p>{skill.source}</p>
    </TooltipContent>
    </Tooltip>
  );
});

export function DiscoverListPanel({
  installedSkillLocations,
  onSelect,
  selectedDetailUrl,
  activeTab,
  onTabChange,
}: DiscoverListPanelProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<DiscoverSkillSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(null);
  const [retryCount, setRetryCount] = useState(0);

  const isSearchActive = query.trim().length > 0;
  const keyboard = useSkillListKeyboard({ compact: true, selectedKey: selectedDetailUrl ?? null,
    contextKey: isSearchActive ? `search:${query}` : `leaderboard:${activeTab}` });
  const searchRef = useRef<HTMLInputElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  const clearSearch = () => {
    setQuery('');
    searchRef.current?.focus({ preventScroll: true });
  };

  useEffect(() => {
    let cancelled = false;

    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
    }

    // 榜单或查询变化时从顶部显示新结果；详情读取不会触发这里。
    if (scrollRef.current) scrollRef.current.scrollTop = 0;
    setResults([]);
    setLoading(true);
    setError(false);

    const loadWithMinimumDelay = async (loader: () => Promise<DiscoverSkillSummary[]>) => {
      const startedAt = Date.now();

      try {
        const data = await loader();
        const remaining = MIN_LOADING_MS - (Date.now() - startedAt);
        if (remaining > 0) {
          await delay(remaining);
        }

        if (cancelled) return;

        setResults(data);
        setError(false);
      } catch {
        if (cancelled) return;

        setResults([]);
        setError(true);
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    };

    if (isSearchActive) {
      debounceRef.current = setTimeout(() => {
        void loadWithMinimumDelay(() => searchDiscoverSkills(query));
      }, 300);
    } else {
      debounceRef.current = setTimeout(() => {
        void loadWithMinimumDelay(() => getDiscoverLeaderboard(activeTab));
      }, LEADERBOARD_LOAD_DELAY_MS);
    }

    return () => {
      cancelled = true;
      if (debounceRef.current) {
        clearTimeout(debounceRef.current);
      }
    };
  }, [query, isSearchActive, activeTab, retryCount]);

  const handleRetry = useCallback(() => {
    searchRef.current?.focus({ preventScroll: true });
    setRetryCount((c) => c + 1);
  }, []);

  return (
    <div {...keyboard} className="discover-list flex flex-col h-full bg-surface">
      <div className="sticky top-0 z-10 bg-surface/95 backdrop-blur border-b px-4 pt-4 pb-0 flex flex-col gap-1 shrink-0">
        <div className="relative">
          <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
          <Input
            ref={searchRef}
            data-skill-search
            type="search"
            name="discover-search"
            aria-label={t('skills.discover.searchLabel')}
            autoComplete="off"
            spellCheck={false}
            onKeyDown={(event) => {
              if (event.key !== 'Escape' || !query || event.nativeEvent.isComposing || event.keyCode === 229) return;
              event.preventDefault();
              event.stopPropagation();
              clearSearch();
            }}
            placeholder={t('skills.discover.searchPlaceholder')}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            className="pl-8 pr-8 h-8 rounded-md bg-accent/50 focus-visible:bg-background shadow-none border-transparent focus-visible:border-ring [&::-webkit-search-cancel-button]:appearance-none"
          />
          {query.length > 0 && (
            <button
              type="button"
              aria-label={t('skills.discover.clearSearch')}
              onClick={clearSearch}
              className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground p-0.5 rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          )}
        </div>

        <div className="grid min-h-9 w-full items-end">
          {!isSearchActive && (
            <Tabs value={activeTab} onValueChange={(value) => onTabChange(value as DiscoverTab)}>
              <TabsList aria-label={t('skills.discover.leaderboards')} style={DISCOVER_TAB_LIST_STYLE} className="flex w-full justify-start gap-6 p-0 bg-transparent h-auto">
                {(['popular', 'trending', 'hot'] as const).map(tab => (
                  <TabsTrigger key={tab} value={tab} style={DISCOVER_TAB_STYLE} className={DISCOVER_TAB_CLASS}>
                    {t(`skills.discover.tabs.${tab}`)}
                  </TabsTrigger>
                ))}
              </TabsList>
            </Tabs>
          )}
          <div role="status" aria-atomic="true" className={isSearchActive ? 'pb-1.5 text-[13px] leading-5 text-muted-foreground' : 'sr-only'}>
            {error ? '' : loading ? t('skills.discover.searching')
              : t(isSearchActive ? 'skills.discover.searchResults' : 'skills.discover.loaded', { count: results.length })}
          </div>
        </div>
      </div>

      <div ref={scrollRef} aria-busy={loading} className="flex-1 overflow-y-auto min-h-0">
        {loading && results.length === 0 ? (
          <div className="p-4 space-y-3" data-testid="discover-list-skeleton">
            {Array.from({ length: 8 }).map((_, i) => (
              <div key={i} className="flex items-center justify-between py-2">
                <div className="space-y-2">
                  <Skeleton className="h-4 w-40" />
                  <Skeleton className="h-3 w-24" />
                </div>
                <Skeleton className="h-4 w-12" />
              </div>
            ))}
          </div>
        ) : error ? (
          <div className="flex flex-col items-center justify-center h-full gap-3 p-4">
            <div role="alert" className="flex items-center gap-2 text-sm text-destructive">
              <AlertCircle className="h-4 w-4" />
              {t(isSearchActive ? 'skills.discover.error' : 'skills.discover.leaderboardError')}
            </div>
            <Button variant="outline" size="sm" onClick={handleRetry}>
              {t('skills.discover.retry')}
            </Button>
          </div>
        ) : results.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full p-4 text-sm text-muted-foreground">
            <p>{t('skills.discover.noResults')}</p>
            {isSearchActive && <p className="text-xs mt-1">{t('skills.discover.noResultsHint')}</p>}
          </div>
        ) : (
          <ul aria-label={t('skills.discover.resultsLabel')} className="space-y-1 px-2 py-2">
            {results.map((skill) => {
              const isInstalled = isSkillInstalled(installedSkillLocations, skill);

              return (
                <li key={skill.detailUrl}>
                <DiscoverSkillItem
                  skill={skill}
                  isInstalled={isInstalled}
                  isSelected={skill.detailUrl === selectedDetailUrl}
                  onSelect={onSelect}
                  t={t}
                />
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}
