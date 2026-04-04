import { useState, useEffect, useRef, useCallback, memo } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { Search, Download, AlertCircle, ShieldCheck, X } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { getDiscoverLeaderboard, searchDiscoverSkills } from '@/lib/discover/api';
import type { DiscoverSkillSummary, DiscoverTab } from '@/lib/discover/types';

const MIN_LOADING_MS = 180;
const LEADERBOARD_LOAD_DELAY_MS = 50;

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function isSkillInstalled(installedSkillKeys: Set<string>, skill: DiscoverSkillSummary): boolean {
  const normalizedSource = skill.source.replace('https://github.com/', '');
  return installedSkillKeys.has(`${skill.source}::${skill.name}`)
    || installedSkillKeys.has(`${normalizedSource}::${skill.name}`);
}

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
  installedSkillKeys: Set<string>;
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
    <div
      className={`flex items-center justify-between p-3 cursor-pointer transition-all duration-200 rounded-lg mx-2 mb-1 border border-transparent ${
        isSelected 
          ? 'bg-accent/60 border-accent/80' 
          : 'hover:bg-accent/40 hover:border-accent/50'
      }`}
      data-testid="discover-skill-item"
      data-selected={isSelected ? 'true' : 'false'}
      onClick={() => onSelect(skill)}
    >
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <div className="font-heading font-semibold text-sm tracking-tight truncate">
            {skill.name}
          </div>
          {skill.isOfficial && (
            <ShieldCheck
              className="h-3 w-3 text-blue-500 shrink-0"
              aria-label={t('skills.discover.official')}
            />
          )}
        </div>
        <div className="text-xs text-muted-foreground truncate">
          {skill.source}
        </div>
      </div>
      <div className="flex items-center gap-3 ml-4 shrink-0">
        {isInstalled && (
          <Badge variant="secondary" className="text-xs">
            {t('skills.discover.installed')}
          </Badge>
        )}
        {skill.displayMetric.rawText ? (
          <span className="inline-flex items-center gap-1.5 text-xs">
            <Download className="h-3 w-3 text-muted-foreground" />
            {renderDisplayMetric(skill.displayMetric)}
          </span>
        ) : null}
      </div>
    </div>
  );
});

export function DiscoverListPanel({
  installedSkillKeys,
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

  useEffect(() => {
    let cancelled = false;

    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
    }

    // Provide immediate feedback by clearing old results when switching modes or tabs
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
    setRetryCount((c) => c + 1);
  }, []);

  return (
    <div className="flex flex-col h-full bg-surface">
      <div className="sticky top-0 z-10 bg-surface/95 backdrop-blur border-b px-4 pt-4 pb-0 flex flex-col gap-1 shrink-0">
        <div className="relative">
          <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
          <Input
            type="text"
            placeholder={t('skills.discover.searchPlaceholder')}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            className="pl-8 pr-8 h-8 rounded-md bg-accent/50 focus-visible:bg-background shadow-none border-transparent focus-visible:border-ring"
          />
          {query.length > 0 && (
            <button
              onClick={() => setQuery('')}
              className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground p-0.5 rounded-sm"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
        
        <div className="grid w-full">
          {/* Normal Browse Mode Tabs */}
          <Tabs
            value={activeTab}
            onValueChange={(val) => {
              if (!isSearchActive) {
                onTabChange(val as DiscoverTab);
              }
            }}
            className={`col-start-1 row-start-1 transition-opacity duration-300 ${isSearchActive ? 'opacity-0 pointer-events-none' : 'opacity-100'}`}
          >
            <TabsList style={{ background: 'transparent', boxShadow: 'none' }} className="flex w-full justify-start gap-6 p-0 bg-transparent h-auto">
              <TabsTrigger 
                value="popular" 
                style={{ background: 'transparent', boxShadow: 'none', borderTop: 'none', borderLeft: 'none', borderRight: 'none' }}
                className="px-0 pt-2 pb-1.5 relative text-[13px] font-medium text-muted-foreground/70 bg-transparent rounded-none !border-x-0 !border-t-0 border-b-[2px] border-transparent mb-[-1px] transition-colors hover:text-foreground data-[state=active]:!bg-transparent data-[state=active]:!shadow-none data-[state=active]:text-foreground data-[state=active]:border-foreground data-[state=active]:font-semibold outline-none ring-0 !ring-offset-0 focus-visible:ring-0"
              >
                {t('skills.discover.tabs.popular')}
              </TabsTrigger>
              <TabsTrigger 
                value="trending" 
                style={{ background: 'transparent', boxShadow: 'none', borderTop: 'none', borderLeft: 'none', borderRight: 'none' }}
                className="px-0 pt-2 pb-1.5 relative text-[13px] font-medium text-muted-foreground/70 bg-transparent rounded-none !border-x-0 !border-t-0 border-b-[2px] border-transparent mb-[-1px] transition-colors hover:text-foreground data-[state=active]:!bg-transparent data-[state=active]:!shadow-none data-[state=active]:text-foreground data-[state=active]:border-foreground data-[state=active]:font-semibold outline-none ring-0 !ring-offset-0 focus-visible:ring-0"
              >
                {t('skills.discover.tabs.trending')}
              </TabsTrigger>
              <TabsTrigger 
                value="hot" 
                style={{ background: 'transparent', boxShadow: 'none', borderTop: 'none', borderLeft: 'none', borderRight: 'none' }}
                className="px-0 pt-2 pb-1.5 relative text-[13px] font-medium text-muted-foreground/70 bg-transparent rounded-none !border-x-0 !border-t-0 border-b-[2px] border-transparent mb-[-1px] transition-colors hover:text-foreground data-[state=active]:!bg-transparent data-[state=active]:!shadow-none data-[state=active]:text-foreground data-[state=active]:border-foreground data-[state=active]:font-semibold outline-none ring-0 !ring-offset-0 focus-visible:ring-0"
              >
                {t('skills.discover.tabs.hot')}
              </TabsTrigger>
            </TabsList>
          </Tabs>

          {/* Search Mode Context Header */}
          <div className={`col-start-1 row-start-1 flex items-end pb-1.5 transition-opacity duration-300 ${isSearchActive ? 'opacity-100' : 'opacity-0 pointer-events-none'}`}>
            <span className="text-[13px] font-medium text-muted-foreground/90 leading-none">
              {loading 
                ? t('skills.discover.searching', '正在搜索...') 
                : t('skills.discover.searchResults', '找到 {{count}} 个相关 skills', { count: results.length })}
            </span>
          </div>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto min-h-0">
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
            <div className="flex items-center gap-2 text-sm text-destructive">
              <AlertCircle className="h-4 w-4" />
              {t('skills.discover.error')}
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
          <div className="py-2">
            {results.map((skill) => {
              const isInstalled = isSkillInstalled(installedSkillKeys, skill);
              
              return (
                <DiscoverSkillItem
                  key={skill.detailUrl}
                  skill={skill}
                  isInstalled={isInstalled}
                  isSelected={skill.detailUrl === selectedDetailUrl}
                  onSelect={onSelect}
                  t={t}
                />
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
