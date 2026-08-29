import { useState, useEffect, useRef, useCallback, memo } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { Search, Download, AlertCircle } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';

import { searchDiscoverSkills } from '@/lib/discover/api';

export interface SearchSkill {
  name: string;
  slug: string;
  source: string;
  installs: number;
}

interface SkillSearchProps {
  /** 已安装的 skill key 集合 (source::name)，用于标记"已安装" */
  installedSkillKeys: Set<string>;
  /** 点击安装按钮的回调，传入选中的搜索结果 */
  onInstall: (skill: SearchSkill) => void;
  /** 调用方提供的动作名称；安装向导未传入时沿用安装文案。 */
  actionLabel?: string;
}

async function searchSkillsAPI(query: string): Promise<SearchSkill[]> {
  const skills = await searchDiscoverSkills(query);
  return skills.map((skill) => ({
    name: skill.name,
    slug: skill.slug,
    source: skill.source || '',
    installs: skill.installs ?? 0,
  }));
}

// [js-hoist-regexp] 纯函数提升到模块顶层，避免每次渲染重新创建
function formatInstalls(count: number): string {
  if (count >= 1000) return `${(count / 1000).toFixed(1)}k`;
  return String(count);
}

// [rerender-memo] 抽取结果行为 memo 组件，避免 50 条列表不必要的 re-render
const SearchResultItem = memo(function SearchResultItem({
  skill,
  isInstalled,
  onInstall,
  actionLabel,
  t,
}: {
  skill: SearchSkill;
  isInstalled: boolean;
  onInstall: (skill: SearchSkill) => void;
  actionLabel?: string;
  t: TFunction;
}) {
  return (
    <div className="flex min-w-0 items-center justify-between gap-3 border-b px-3 py-2.5 last:border-b-0 hover:bg-muted/35">
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-medium">
          {skill.name}
        </div>
        <div className="mt-0.5 truncate text-xs text-muted-foreground" translate="no">
          {skill.source}
        </div>
      </div>
      <div className="ml-2 flex shrink-0 items-center gap-3">
        {skill.installs > 0 ? (
          <span className="flex items-center text-xs text-muted-foreground">
            <Download className="mr-1 size-3.5 opacity-70" aria-hidden="true" />
            {formatInstalls(skill.installs)}
          </span>
        ) : null}
        {isInstalled ? (
          <Badge variant="secondary" className="text-xs font-medium bg-secondary/50">
            {t('skills.discover.installed')}
          </Badge>
        ) : (
          <Button
            variant="outline"
            size="sm"
            className="h-7 px-2.5 text-xs"
            onClick={() => onInstall(skill)}
          >
            {actionLabel ?? t('skills.discover.install')}
          </Button>
        )}
      </div>
    </div>
  );
});

export function SkillSearch({ installedSkillKeys, onInstall, actionLabel }: SkillSearchProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<SearchSkill[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(null);
  const searchRequestIdRef = useRef(0);
  // [rerender-move-effect-to-event] 用 retryCount 驱动 effect 重新搜索，
  // 避免 handleRetry 捕获 query 闭包 (rerender-defer-reads)
  const [retryCount, setRetryCount] = useState(0);

  // 防抖搜索 — retryCount 变化也会触发重新搜索
  useEffect(() => {
    const requestId = ++searchRequestIdRef.current;

    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
    }

    if (!query || query.length < 2) {
      setResults([]);
      setLoading(false);
      setError(false);
      return;
    }

    setLoading(true);
    setError(false);

    debounceRef.current = setTimeout(async () => {
      try {
        const data = await searchSkillsAPI(query);
        if (requestId !== searchRequestIdRef.current) return;

        setResults(data);
        setError(false);
      } catch {
        if (requestId !== searchRequestIdRef.current) return;

        setResults([]);
        setError(true);
      } finally {
        if (requestId === searchRequestIdRef.current) {
          setLoading(false);
        }
      }
    }, 300);

    return () => {
      if (debounceRef.current) {
        clearTimeout(debounceRef.current);
      }
    };
  }, [query, retryCount]);

  // [rerender-defer-reads] handleRetry 零外部依赖，引用完全稳定
  const handleRetry = useCallback(() => {
    setRetryCount((c) => c + 1);
  }, []);

  return (
    <div className="flex flex-col h-full">
      {/* 搜索框 */}
      <div className="relative mb-3 shrink-0">
        <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden="true" />
        <Input
          type="search"
          name="online-skill-search"
          autoComplete="off"
          spellCheck={false}
          aria-label={t('skills.discover.searchPlaceholder')}
          placeholder={t('skills.discover.searchPlaceholder')}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="h-9 pl-9"
        />
      </div>

      {/* 结果区域 */}
      <div className="min-h-0 flex-1 overflow-auto overscroll-contain rounded-md border bg-background">
        {/* 初始提示 */}
        {!query || query.length < 2 ? (
          <div className="flex items-center justify-center h-32 text-sm text-muted-foreground">
            {t('skills.discover.minCharsHint')}
          </div>
        ) : loading && results.length === 0 ? (
          /* 加载中骨架屏 */
          <div className="space-y-3 p-3">
            {Array.from({ length: 4 }).map((_, i) => (
              <div key={i} className="flex items-center justify-between py-2">
                <div className="space-y-2">
                  <Skeleton className="h-4 w-40" />
                  <Skeleton className="h-3 w-24" />
                </div>
                <Skeleton className="h-8 w-16" />
              </div>
            ))}
          </div>
        ) : error ? (
          /* 错误状态 */
          <div className="flex flex-col items-center justify-center h-32 gap-3">
            <div className="flex items-center gap-2 text-sm text-destructive">
              <AlertCircle className="h-4 w-4" />
              {t('skills.discover.error')}
            </div>
            <Button variant="outline" size="sm" onClick={handleRetry}>
              {t('skills.discover.retry')}
            </Button>
          </div>
        ) : results.length === 0 ? (
          /* 空结果 */
          <div className="flex flex-col items-center justify-center h-32 text-sm text-muted-foreground">
            <p>{t('skills.discover.noResults')}</p>
            <p className="text-xs mt-1">{t('skills.discover.noResultsHint')}</p>
          </div>
        ) : (
          /* 结果列表 — 使用 memo 化的 SearchResultItem */
          <div>
            {results.map((skill) => (
              <SearchResultItem
                key={skill.slug}
                skill={skill}
                isInstalled={installedSkillKeys.has(`${skill.source}::${skill.name}`)}
                onInstall={onInstall}
                actionLabel={actionLabel}
                t={t}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
