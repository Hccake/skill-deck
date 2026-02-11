// src/components/skills/add-skill/AgentSelector.tsx
import { useMemo, memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Checkbox } from '@/components/ui/checkbox';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import { AgentBadges } from './AgentBadges';
import { UniversalAgentsCard } from './UniversalAgentsCard';
import type { AgentItem } from './types';

interface AgentSelectorProps {
  /** 选中的 agent IDs */
  selectedAgents: string[];
  /** 所有 agents */
  allAgents: AgentItem[];
  /** 选择变化回调 */
  onSelectionChange: (agents: string[]) => void;
  /** 安装范围（用于动态显示 Universal 路径） */
  scope?: 'global' | 'project';
  /** 是否支持折叠（默认 false） */
  collapsible?: boolean;
  /** 默认是否折叠 */
  collapsed?: boolean;
  /** 折叠状态变化回调 */
  onCollapsedChange?: (collapsed: boolean) => void;
}

export function AgentSelector({
  selectedAgents,
  allAgents,
  onSelectionChange,
  scope,
  collapsible = false,
  collapsed = false,
  onCollapsedChange,
}: AgentSelectorProps) {
  const { t } = useTranslation();

  // 分组：Universal / 已检测 / 其他
  const { universalAgents, detectedAgents, otherAgents } = useMemo(() => {
    const universal: AgentItem[] = [];
    const detected: AgentItem[] = [];
    const other: AgentItem[] = [];

    for (const agent of allAgents) {
      if (agent.isUniversal && agent.showInUniversalList) {
        // Universal Agents（显示在卡片中）
        universal.push(agent);
      } else if (!agent.isUniversal) {
        // Non-Universal Agents
        if (agent.detected) {
          detected.push(agent);
        } else {
          other.push(agent);
        }
      }
      // isUniversal && !showInUniversalList 的 agents 不显示（如 Replit）
    }

    return { universalAgents: universal, detectedAgents: detected, otherAgents: other };
  }, [allAgents]);

  // 切换单个 agent
  const toggleAgent = (agentId: string) => {
    const isSelected = selectedAgents.includes(agentId);
    const newSelection = isSelected
      ? selectedAgents.filter((id) => id !== agentId)
      : [...selectedAgents, agentId];
    onSelectionChange(newSelection);
  };

  // 是否有可选的 agents
  const hasSelectableAgents = detectedAgents.length > 0 || otherAgents.length > 0;

  // 渲染主要内容
  const renderContent = () => (
    <div className="space-y-4">
      {/* Universal Agents 卡片（只读信息） */}
      <UniversalAgentsCard
        universalAgents={universalAgents}
        scope={scope}
      />

      {/* 可选 Agents */}
      {hasSelectableAgents && (
        <div className="space-y-3">
          {/* 已检测 Agents */}
          {detectedAgents.length > 0 && (
            <div className="space-y-1">
              <div className="flex items-center gap-2">
                <span className="text-xs text-muted-foreground font-medium">
                  {t('addSkill.agents.detectedSection')}
                </span>
                <Separator className="flex-1" />
              </div>
              <div className="space-y-1">
                {detectedAgents.map((agent) => (
                  <AgentRow
                    key={agent.id}
                    agent={agent}
                    selected={selectedAgents.includes(agent.id)}
                    onToggle={() => toggleAgent(agent.id)}
                    showDetectedBadge
                  />
                ))}
              </div>
            </div>
          )}

          {/* 其他 Agents */}
          {otherAgents.length > 0 && (
            <div className="space-y-1">
              <div className="flex items-center gap-2">
                <span className="text-xs text-muted-foreground font-medium">
                  {t('addSkill.agents.otherSection')}
                </span>
                <Separator className="flex-1" />
              </div>
              <div className="space-y-1">
                {otherAgents.map((agent) => (
                  <AgentRow
                    key={agent.id}
                    agent={agent}
                    selected={selectedAgents.includes(agent.id)}
                    onToggle={() => toggleAgent(agent.id)}
                  />
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );

  // 可折叠模式 - Universal Agents 始终显示，只有 Non-Universal 部分可折叠
  if (collapsible) {
    return (
      <div className="space-y-3">
        <h4 className="text-sm font-medium">{t('addSkill.agents.title')}</h4>

        {/* Universal Agents - 永远显示 */}
        <UniversalAgentsCard universalAgents={universalAgents} scope={scope} />

        {/* Non-Universal Agents - 可折叠 */}
        {hasSelectableAgents && (
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <span className="text-xs text-muted-foreground font-medium">
                {t('addSkill.agents.otherAgentsTitle')}
              </span>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => onCollapsedChange?.(!collapsed)}
              >
                {collapsed
                  ? t('addSkill.agents.expand')
                  : t('addSkill.agents.collapse')}
              </Button>
            </div>

            {collapsed ? (
              // 折叠状态：显示已选 agents 的 badges 摘要
              <div
                className="p-3 border rounded-md cursor-pointer hover:bg-muted/50"
                onClick={() => onCollapsedChange?.(false)}
              >
                <AgentBadges
                  selectedAgents={selectedAgents}
                  allAgents={allAgents}
                  excludeUniversal
                />
              </div>
            ) : (
              // 展开状态：显示 Non-Universal agents 列表
              <div className="space-y-3">
                {/* 已检测 Agents */}
                {detectedAgents.length > 0 && (
                  <div className="space-y-1">
                    <div className="flex items-center gap-2">
                      <span className="text-xs text-muted-foreground">
                        {t('addSkill.agents.detectedSection')}
                      </span>
                      <Separator className="flex-1" />
                    </div>
                    <div className="space-y-1">
                      {detectedAgents.map((agent) => (
                        <AgentRow
                          key={agent.id}
                          agent={agent}
                          selected={selectedAgents.includes(agent.id)}
                          onToggle={() => toggleAgent(agent.id)}
                          showDetectedBadge
                        />
                      ))}
                    </div>
                  </div>
                )}

                {/* 其他 Agents */}
                {otherAgents.length > 0 && (
                  <div className="space-y-1">
                    <div className="flex items-center gap-2">
                      <span className="text-xs text-muted-foreground">
                        {t('addSkill.agents.otherSection')}
                      </span>
                      <Separator className="flex-1" />
                    </div>
                    <div className="space-y-1">
                      {otherAgents.map((agent) => (
                        <AgentRow
                          key={agent.id}
                          agent={agent}
                          selected={selectedAgents.includes(agent.id)}
                          onToggle={() => toggleAgent(agent.id)}
                        />
                      ))}
                    </div>
                  </div>
                )}
              </div>
            )}
          </div>
        )}
      </div>
    );
  }

  // 非折叠模式（Settings 页面使用）
  return (
    <div className="space-y-3">
      {renderContent()}
    </div>
  );
}

// 单个 agent 行
const AgentRow = memo(function AgentRow({
  agent,
  selected,
  onToggle,
  showDetectedBadge = false,
}: {
  agent: AgentItem;
  selected: boolean;
  onToggle: () => void;
  showDetectedBadge?: boolean;
}) {
  const { t } = useTranslation();

  return (
    <div
      className="flex items-center gap-3 p-2 rounded-md hover:bg-muted/50 cursor-pointer"
      onClick={onToggle}
    >
      <Checkbox checked={selected} />
      <div className="flex-1 min-w-0">
        <span className="text-sm">{agent.displayName}</span>
      </div>
      {showDetectedBadge && agent.detected && (
        <Badge variant="outline" className="text-xs">
          {t('addSkill.agents.detected')}
        </Badge>
      )}
    </div>
  );
});
