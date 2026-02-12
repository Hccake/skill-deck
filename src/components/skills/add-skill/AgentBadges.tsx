// src/components/skills/add-skill/AgentBadges.tsx
import { useTranslation } from 'react-i18next';
import { Badge } from '@/components/ui/badge';
import type { AgentInfo } from '@/bindings';

interface AgentBadgesProps {
  /** 选中的 agent IDs */
  selectedAgents: string[];
  /** 所有 agents（用于获取 name） */
  allAgents: AgentInfo[];
  /** 是否排除 Universal Agents（折叠时 Universal 已单独显示） */
  excludeUniversal?: boolean;
}

/**
 * 用于折叠状态下显示选中的 agents 标签
 */
export function AgentBadges({
  selectedAgents,
  allAgents,
  excludeUniversal = false,
}: AgentBadgesProps) {
  const { t } = useTranslation();
  // 过滤掉 Universal Agents（如果需要）
  const filteredAgents = excludeUniversal
    ? allAgents.filter((a) => !a.isUniversal)
    : allAgents;

  // 构建 id -> name 映射
  const agentNames = new Map<string, string>(filteredAgents.map((a) => [a.id, a.name]));

  // 只显示在过滤后列表中的已选 agents
  const displayAgents = excludeUniversal
    ? selectedAgents.filter((id) => filteredAgents.some((a) => a.id === id))
    : selectedAgents;

  return (
    <div className="flex flex-wrap gap-1.5">
      {displayAgents.map((agentId) => (
        <Badge key={agentId} variant="secondary" className="text-xs">
          {agentNames.get(agentId) ?? agentId}
        </Badge>
      ))}
      {displayAgents.length === 0 && (
        <span className="text-sm text-muted-foreground">{t('addSkill.agents.noneSelected')}</span>
      )}
    </div>
  );
}
