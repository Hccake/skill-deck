import { useMemo, useCallback, useEffect, useRef, memo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Checkbox } from '@/components/ui/checkbox';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { getAgentTarget, isAdditionalAgent, isAutomaticAgent } from '@/lib/agentTargets';
import { cn } from '@/lib/utils';
import type { AgentInfo } from '@/bindings';

interface AgentSelectorProps {
  /** 选中的 agent IDs */
  selectedAgents: string[];
  /** 所有 agents */
  allAgents: AgentInfo[];
  /** 选择变化回调 */
  onSelectionChange: (agents: string[]) => void;
  /** 安装范围（用于动态显示自动应用路径） */
  scope?: 'global' | 'project';
}

export function AgentSelector({
  selectedAgents,
  allAgents,
  onSelectionChange,
  scope = 'global',
}: AgentSelectorProps) {
  const { t } = useTranslation();
  const selectedAgentsRef = useRef(selectedAgents);
  const [isExpanded, setIsExpanded] = useState(false);

  useEffect(() => {
    selectedAgentsRef.current = selectedAgents;
  }, [selectedAgents]);

  const selectedAgentIds = useMemo(() => new Set(selectedAgents), [selectedAgents]);

  const { automaticAgents, detectedAgents, otherAgents } = useMemo(() => {
    const automatic: AgentInfo[] = [];
    const detected: AgentInfo[] = [];
    const other: AgentInfo[] = [];

    for (const agent of allAgents) {
      if (isAutomaticAgent(agent, scope)) {
        automatic.push(agent);
      } else if (isAdditionalAgent(agent, scope)) {
        if (agent.detected) {
          detected.push(agent);
        } else {
          other.push(agent);
        }
      }
    }

    return { automaticAgents: automatic, detectedAgents: detected, otherAgents: other };
  }, [allAgents, scope]);

  const toggleAgent = useCallback((agentId: string) => {
    const currentSelection = selectedAgentsRef.current;
    const isSelected = currentSelection.includes(agentId);
    const newSelection = isSelected
      ? currentSelection.filter((id) => id !== agentId)
      : [...currentSelection, agentId];
    selectedAgentsRef.current = newSelection;
    onSelectionChange(newSelection);
  }, [onSelectionChange]);

  const hasSelectableAgents = detectedAgents.length > 0 || otherAgents.length > 0;

  const getAutomaticPath = () => {
    if (scope === 'global') return '~/.agents/skills/';
    if (scope === 'project') return './.agents/skills/';
    return '.agents/skills/';
  };

  return (
    <div className="space-y-6 pt-1">
      {automaticAgents.length > 0 && (
        <div className="space-y-2.5">
          <div className="flex items-center gap-2.5">
            <span className="text-[13px] font-semibold text-foreground tracking-tight">
              {t('addSkill.agents.automaticTitle', 'Applied automatically')}
            </span>
            <code className="text-[11px] text-muted-foreground/70 bg-muted/60 px-1.5 py-0.5 rounded font-mono truncate">
              {getAutomaticPath()}
            </code>
          </div>
          <div className="flex flex-wrap gap-x-5 gap-y-2.5 pt-0.5">
            {automaticAgents.map((agent) => (
              <div key={agent.id} className="text-[13px] text-muted-foreground flex items-center">
                <span className="bg-emerald-500/80 w-1.5 h-1.5 rounded-full mr-2 shrink-0"></span>
                <span className="font-medium text-foreground/80">{agent.name}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {hasSelectableAgents && (
        <div className="space-y-2">
          <div className="flex items-center gap-2 pb-1">
            <span className="text-[13px] font-semibold text-foreground">
              {t('addSkill.agents.additionalTitle', 'Manual selection')}
            </span>
            <span className="text-[11px] text-muted-foreground/70 truncate">
              {t('addSkill.agents.additionalHint')}
            </span>
          </div>

          <div className="flex flex-col space-y-0.5">
            {detectedAgents.map((agent) => (
              <AgentRow
                key={agent.id}
                agent={agent}
                selected={selectedAgentIds.has(agent.id)}
                onToggle={toggleAgent}
                showDetectedBadge
                scope={scope}
              />
            ))}
          </div>

          {otherAgents.length > 0 && (
            <Collapsible open={isExpanded} onOpenChange={setIsExpanded}>
              <CollapsibleTrigger asChild>
                <Button
                  variant="ghost"
                  size="sm"
                  className="w-full justify-center text-xs text-muted-foreground/70 hover:bg-accent/40 hover:text-foreground transition-colors h-8"
                >
                  {isExpanded
                    ? t('addSkill.agents.collapseOtherAgents', '↑ Collapse')
                    : t('addSkill.agents.expandOtherAgents', { count: otherAgents.length })}
                </Button>
              </CollapsibleTrigger>
              <CollapsibleContent className="flex flex-col space-y-0.5 mt-0.5">
                {otherAgents.map((agent) => (
                  <AgentRow
                    key={agent.id}
                    agent={agent}
                    selected={selectedAgentIds.has(agent.id)}
                    onToggle={toggleAgent}
                    scope={scope}
                    className="opacity-75 hover:opacity-100"
                  />
                ))}
              </CollapsibleContent>
            </Collapsible>
          )}
        </div>
      )}
    </div>
  );
}

const AgentRow = memo(function AgentRow({
  agent,
  selected,
  onToggle,
  showDetectedBadge = false,
  scope,
  className,
}: {
  agent: AgentInfo;
  selected: boolean;
  onToggle: (agentId: string) => void;
  showDetectedBadge?: boolean;
  scope?: 'global' | 'project';
  className?: string;
}) {
  const { t } = useTranslation();

  const target = scope ? getAgentTarget(agent, scope) : null;
  const pathLabel = target
    ? scope === 'project'
      ? `./${target.path}`
      : target.path
    : undefined;

  return (
    <div
      className={cn(
        "flex items-center gap-3 px-3 py-2 cursor-pointer transition-all duration-200 border rounded-md",
        selected
          ? "bg-accent/60 border-accent/80"
          : "hover:bg-accent/40 hover:border-accent/50 border-transparent",
        className
      )}
      onClick={() => onToggle(agent.id)}
    >
      <Checkbox checked={selected} className="shrink-0" />
      <div className="flex-1 flex items-center min-w-0 gap-2.5">
        <span className="text-[13px] font-medium leading-none tracking-tight">{agent.name}</span>
        {pathLabel && (
          <code className="text-[11px] text-muted-foreground/70 font-mono truncate">
            {pathLabel}
          </code>
        )}
      </div>
      {showDetectedBadge && agent.detected && (
        <Badge variant="secondary" className="text-[10px] h-5 px-1.5 rounded-sm font-normal bg-accent/60 hover:bg-accent/60">
          {t('settings.detected', 'Installed')}
        </Badge>
      )}
    </div>
  );
});
