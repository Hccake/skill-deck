import { useEffect, useState, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { Target, Info } from 'lucide-react';
import { Skeleton } from '@/components/ui/skeleton';
import { listAgents, getLastSelectedAgents, saveLastSelectedAgents } from '@/hooks/useTauriApi';
import { AgentSelector } from '@/components/skills/add-skill/AgentSelector';
import type { AgentInfo } from '@/bindings';

export function GeneralTab() {
  const { t } = useTranslation();

  const [allAgents, setAllAgents] = useState<AgentInfo[]>([]);
  const [selectedAgents, setSelectedAgents] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);

  // 加载 agents 数据和默认选择
  useEffect(() => {
    async function fetchData() {
      try {
        setLoading(true);
        const [agentsData, lastSelected] = await Promise.all([
          listAgents(),
          getLastSelectedAgents(),
        ]);
        setAllAgents(agentsData);
        setSelectedAgents(lastSelected);
      } catch (e) {
        console.error('Failed to load data:', e);
      } finally {
        setLoading(false);
      }
    }
    fetchData();
  }, []);

  // 处理 agents 选择变化
  const handleSelectionChange = useCallback((agents: string[]) => {
    setSelectedAgents(agents);
    // 异步保存
    saveLastSelectedAgents(agents).catch((error) => {
      console.error('Failed to save agents:', error);
    });
  }, []);

  // 检查是否有 Non-Universal agents
  const hasNonUniversalAgents = allAgents.some((a) => !a.isUniversal);

  return (
    <div className="space-y-5 sm:space-y-6">
      <section>
        <div className="flex items-center gap-2 sm:gap-2.5 mb-3">
          <div className="flex h-7 w-7 items-center justify-center rounded-lg bg-accent">
            <Target className="h-4 w-4 text-accent-foreground" />
          </div>
          <div>
            <h2 className="text-sm font-heading font-bold text-foreground">
              {t('settings.defaultAgents.title')}
            </h2>
            <p className="text-xs text-muted-foreground">
              {t('settings.defaultAgents.description')}
            </p>
          </div>
        </div>

        {loading ? (
          <div className="space-y-2 sm:space-y-3 animate-in fade-in duration-300">
            {Array.from({ length: 3 }).map((_, i) => (
              <div key={i} className="flex items-center gap-3 p-3 sm:p-4 rounded-xl border border-border/40 bg-accent/10">
                <Skeleton className="h-10 w-10 rounded-lg shrink-0" />
                <div className="flex-1 space-y-2">
                  <Skeleton className="h-4 w-1/3 max-w-[120px]" />
                  <Skeleton className="h-3 w-1/2 max-w-[200px]" />
                </div>
              </div>
            ))}
          </div>
        ) : !hasNonUniversalAgents ? (
          <div className="relative overflow-hidden rounded-xl border border-dashed border-border/80 bg-accent/20 p-5 sm:p-6">
            <div className="flex flex-col items-center text-center">
              <div className="flex h-10 w-10 items-center justify-center rounded-full bg-muted mb-2.5">
                <Target className="h-5 w-5 text-muted-foreground" />
              </div>
              <p className="text-sm font-medium text-foreground mb-1">
                {t('settings.defaultAgents.empty')}
              </p>
              <p className="text-xs text-muted-foreground max-w-[220px]">
                {t('settings.defaultAgents.emptyHint')}
              </p>
            </div>
          </div>
        ) : (
          <div className="space-y-3">
            {/* 复用 AgentSelector 组件 */}
            <AgentSelector
              selectedAgents={selectedAgents}
              allAgents={allAgents}
              onSelectionChange={handleSelectionChange}
            />

            {/* CLI 共享提示 */}
            <p className="text-xs text-muted-foreground flex items-center gap-1.5">
              <Info className="h-3 w-3" />
              {t('settings.defaultAgents.cliShared')}
            </p>
          </div>
        )}
      </section>
    </div>
  );
}
