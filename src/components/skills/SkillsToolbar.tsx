// src/components/skills/SkillsToolbar.tsx
import { useState, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { Search, RefreshCw, Check } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import type { AgentInfo } from '@/bindings';

interface SkillsToolbarProps {
  /** 紧凑模式 — 只显示搜索框 */
  compact?: boolean;
  /** 搜索关键词 */
  searchQuery: string;
  /** 搜索关键词变更回调 */
  onSearchChange: (query: string) => void;
  /** 当前选中的 agent 筛选值 */
  selectedAgent: string;
  /** agent 筛选变更回调 */
  onAgentChange: (agentId: string) => void;
  /** 可筛选的 agent 列表 */
  filterableAgents: AgentInfo[];
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

  // 紧凑模式：只显示搜索框
  if (compact) {
    return (
      <div className="relative">
        <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground/50" />
        <Input
          type="text"
          placeholder={t('skills.search')}
          value={searchQuery}
          onChange={(e) => onSearchChange(e.target.value)}
          className="h-8 pl-8 text-sm"
        />
      </div>
    );
  }

  return (
    <div className="flex items-center gap-3 mb-4">
      {/* Search Input */}
      <div className="relative flex-1">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
        <Input
          type="text"
          placeholder={t('skills.search')}
          value={searchQuery}
          onChange={(e) => onSearchChange(e.target.value)}
          className="pl-9 h-9"
        />
      </div>

      {/* Agent Filter */}
      {filterableAgents.length > 0 && (
        <Select value={selectedAgent} onValueChange={onAgentChange}>
          <SelectTrigger size="sm" className="h-9 min-w-[130px]">
            <SelectValue placeholder={t('skills.filter.allAgents')} />
          </SelectTrigger>
          <SelectContent position="popper">
            <SelectItem value="all">{t('skills.filter.allAgents')}</SelectItem>
            {filterableAgents.map((agent) => (
              <SelectItem key={agent.id} value={agent.id}>
                {agent.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}

      {/* Sync Button */}
      <Button
        variant="outline"
        size="sm"
        className="h-9 gap-2"
        onClick={handleSync}
        disabled={isBusy}
      >
        {syncStatus === 'done'
          ? <Check className="h-4 w-4 text-success" />
          : <RefreshCw className={`h-4 w-4 ${isBusy ? 'animate-spin' : ''}`} />
        }
        {t('skills.sync')}
      </Button>
    </div>
  );
}
