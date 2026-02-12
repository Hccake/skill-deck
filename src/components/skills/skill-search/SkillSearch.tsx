import { useState, useEffect, useRef, useCallback, memo } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { fetch } from '@tauri-apps/plugin-http';
import { Search, Download, AlertCircle } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';

const SEARCH_API_BASE = 'https://skills.sh';

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
}

async function searchSkillsAPI(query: string): Promise<SearchSkill[]> {
  const url = `${SEARCH_API_BASE}/api/search?q=${encodeURIComponent(query)}&limit=50`;
  const res = await fetch(url);
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const data = (await res.json()) as {
    skills: Array<{
      id: string;
      name: string;
      installs: number;
      source: string;
    }>;
  };
  return data.skills.map((skill) => ({
    name: skill.name,
    slug: skill.id,
    source: skill.source || '',
    installs: skill.installs,
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
  t,
}: {
  skill: SearchSkill;
  isInstalled: boolean;
  onInstall: (skill: SearchSkill) => void;
  t: TFunction;
}) {
  return (
    <div className="flex items-center justify-between py-2.5 px-1">
      <div className="min-w-0 flex-1">
        <div className="font-medium text-sm truncate">
          {skill.name}
        </div>
        <div className="text-xs text-muted-foreground truncate">
          {skill.source}
        </div>
      </div>
      <div className="flex items-center gap-3 ml-4 shrink-0">
        {skill.installs > 0 ? (
          <span className="text-xs text-muted-foreground">
            <Download className="inline h-3 w-3 mr-1" />
            {formatInstalls(skill.installs)}
          </span>
        ) : null}
        {isInstalled ? (
          <Badge variant="secondary" className="text-xs">
            {t('skills.discover.installed')}
          </Badge>
        ) : (
          <Button
            variant="outline"
            size="sm"
            className="h-7 text-xs"
            onClick={() => onInstall(skill)}
          >
            {t('skills.discover.install')}
          </Button>
        )}
      </div>
    </div>
  );
});

export function SkillSearch({ installedSkillKeys, onInstall }: SkillSearchProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<SearchSkill[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(null);
  // [rerender-move-effect-to-event] 用 retryCount 驱动 effect 重新搜索，
  // 避免 handleRetry 捕获 query 闭包 (rerender-defer-reads)
  const [retryCount, setRetryCount] = useState(0);

  // 防抖搜索 — retryCount 变化也会触发重新搜索
  useEffect(() => {
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
        setResults(data);
        setError(false);
      } catch {
        setResults([]);
        setError(true);
      } finally {
        setLoading(false);
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
      <div className="relative mb-4">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
        <Input
          type="text"
          placeholder={t('skills.discover.searchPlaceholder')}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="pl-9 h-9"
        />
      </div>

      {/* 结果区域 */}
      <div className="flex-1 overflow-auto min-h-0">
        {/* 初始提示 */}
        {!query || query.length < 2 ? (
          <div className="flex items-center justify-center h-32 text-sm text-muted-foreground">
            {t('skills.discover.minCharsHint')}
          </div>
        ) : loading && results.length === 0 ? (
          /* 加载中骨架屏 */
          <div className="space-y-3">
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
          <div className="divide-y">
            {results.map((skill) => (
              <SearchResultItem
                key={skill.slug}
                skill={skill}
                isInstalled={installedSkillKeys.has(`${skill.source}::${skill.name}`)}
                onInstall={onInstall}
                t={t}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
