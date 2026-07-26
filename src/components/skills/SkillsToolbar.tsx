// src/components/skills/SkillsToolbar.tsx
import { useState, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { Search, RefreshCw, Check, X } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { AgentFilterCombobox } from './AgentFilterCombobox';
import type { AgentId, ResolvedAgent } from '@/bindings';

interface SkillsToolbarProps {
  /** 紧凑模式 */
  compact?: boolean;
  /** 搜索关键词 */
  searchQuery: string;
  /** 搜索关键词变更回调 */
  onSearchChange: (query: string) => void;
  /** 当前选中的 Agent 筛选值 */
  selectedAgent: AgentId | null;
  /** agent 筛选变更回调 */
  onAgentChange: (agentId: AgentId | null) => void;
  /** 可筛选的 agent 列表 */
  filterableAgents: ResolvedAgent[];
  /** 每个 Agent 可匹配的 Skill 数量 */
  agentMatchCounts: ReadonlyMap<AgentId, number>;
  /** 当前 Context 中的 Skill 总数 */
  totalSkillCount: number;
  /** 是否存在搜索或 Agent 筛选 */
  hasActiveFilters: boolean;
  /** 清除所有筛选条件 */
  onClearFilters: () => void;
  /** 同步按钮回调 */
  onSync: () => void | Promise<void>;
  /** 是否正在同步 */
  isSyncing?: boolean;
}

export function SkillsToolbar({
  compact = false,
  searchQuery,
  onSearchChange,
  selectedAgent,
  onAgentChange,
  filterableAgents,
  agentMatchCounts,
  totalSkillCount,
  hasActiveFilters,
  onClearFilters,
  onSync,
  isSyncing = false,
}: SkillsToolbarProps) {
  const { t } = useTranslation();

  // local state: 最小 spin 时间 + ✓ 完成态闪现
  const [syncStatus, setSyncStatus] = useState<'idle' | 'syncing' | 'done'>('idle');
  const isBusy = isSyncing || syncStatus !== 'idle';

  const handleSync = useCallback(async () => {
    if (isBusy) return;
    setSyncStatus('syncing');
    const minDelay = new Promise<void>((r) => setTimeout(r, 300));
    await Promise.all([onSync(), minDelay]);
    setSyncStatus('done');
    setTimeout(() => setSyncStatus('idle'), 800);
  }, [isBusy, onSync]);

  return (
    <div className={cn(
      'flex flex-wrap items-center gap-2',
      compact ? 'mb-0' : 'mb-4 gap-3',
    )}>
      {/* Search Input */}
      <div className="relative min-w-40 flex-1">
        <Search className={cn(
          'pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground',
          compact ? 'size-3.5 opacity-70' : 'size-4',
        )} aria-hidden="true" />
        <Input
          type="search"
          aria-label={t('skills.search')}
          name="skill-search"
          autoComplete="off"
          placeholder={t('skills.search')}
          value={searchQuery}
          onChange={(e) => onSearchChange(e.target.value)}
          onKeyDown={(event) => {
            if (event.key !== 'Escape' || searchQuery.length === 0) return;
            event.preventDefault();
            event.stopPropagation();
            onSearchChange('');
          }}
          className="h-8 border-transparent bg-muted pl-8 text-sm shadow-none transition-colors hover:bg-accent focus-visible:bg-background focus-visible:ring-1"
        />
      </div>

      {/* Agent Filter */}
      {(filterableAgents.length > 0 || selectedAgent !== null) && (
        <AgentFilterCombobox
          agents={filterableAgents}
          selectedAgent={selectedAgent}
          onChange={onAgentChange}
          matchCounts={agentMatchCounts}
          totalSkillCount={totalSkillCount}
          compact={compact}
        />
      )}

      {hasActiveFilters && (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="size-8 p-0 text-muted-foreground"
          aria-label={t('skills.filter.clear')}
          title={t('skills.filter.clear')}
          onClick={onClearFilters}
        >
          <X className="size-3.5" aria-hidden="true" />
        </Button>
      )}

      {/* Sync Button */}
      {!compact && <Button
        type="button"
        variant="secondary"
        size="sm"
        className="h-8 gap-2 shadow-none bg-muted hover:bg-accent text-foreground border border-transparent transition-colors"
        onClick={handleSync}
        disabled={isBusy}
      >
        {syncStatus === 'done'
          ? <Check className="h-4 w-4 text-success" aria-hidden="true" />
          : <RefreshCw className={`h-4 w-4 ${isBusy ? 'animate-spin' : ''}`} aria-hidden="true" />
        }
        {t('skills.sync')}
      </Button>}
    </div>
  );
}
