// src/components/skills/add-skill/UniversalAgentsCard.tsx
import { memo } from 'react';
import { FolderOpen } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import type { AgentInfo } from '@/bindings';

interface UniversalAgentsCardProps {
  /** Universal Agents 列表 */
  universalAgents: AgentInfo[];
  /** 安装范围（用于动态显示路径） */
  scope?: 'global' | 'project';
}

/**
 * Universal Agents 信息卡片
 * 显示共享目录路径和自动生效的 agents 列表
 */
export const UniversalAgentsCard = memo(function UniversalAgentsCard({
  universalAgents,
  scope,
}: UniversalAgentsCardProps) {
  // 如果没有 Universal Agents，不渲染
  if (universalAgents.length === 0) {
    return null;
  }

  // 根据 scope 确定显示的路径
  const getPath = () => {
    if (scope === 'global') {
      return '~/.agents/skills/';
    }
    if (scope === 'project') {
      return './.agents/skills/';
    }
    // 默认（设置页面）显示通用形式
    return '.agents/skills/';
  };

  return (
    <div className="rounded-lg border bg-muted/30 p-3">
      {/* 路径标题 */}
      <div className="flex items-center gap-2 mb-2">
        <FolderOpen className="h-4 w-4 text-muted-foreground" />
        <code className="text-sm font-mono text-foreground">
          {getPath()}
        </code>
      </div>

      {/* Universal Agents badges */}
      <div className="flex flex-wrap gap-1.5">
        {universalAgents.map((agent) => (
          <Badge
            key={agent.id}
            variant="secondary"
            className="text-xs font-normal"
          >
            {agent.name}
          </Badge>
        ))}
      </div>
    </div>
  );
});
