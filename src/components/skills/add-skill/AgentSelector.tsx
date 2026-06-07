import { useMemo, useCallback, useEffect, useRef, memo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Checkbox } from '@/components/ui/checkbox';
import { Button } from '@/components/ui/button';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { ChevronDown, ChevronUp } from 'lucide-react';
import {
  formatAgentTargetPath,
  groupAgentsByScopedTarget,
  getAgentTarget,
  getSharedSkillDirectory,
} from '@/lib/agentTargets';
import { AgentIcon } from '@/components/ui/agent-icon';
import { cn } from '@/lib/utils';
import type { AgentInfo } from '@/bindings';

interface AgentSelectorProps {
  /** 选中的 agent IDs */
  selectedAgents: string[];
  /** 明确创建独立副本的默认可用 agent IDs */
  privateCopyAgents?: string[];
  /** 不应展示为可创建独立副本的 agent IDs */
  excludedPrivateCopyAgents?: string[];
  /** 所有 agents */
  allAgents: AgentInfo[];
  /** 选择变化回调 */
  onSelectionChange: (agents: string[]) => void;
  /** 独立副本选择变化回调 */
  onPrivateCopyChange?: (agents: string[]) => void;
  /** 安装范围（用于动态显示自动应用路径） */
  scope?: 'global' | 'project';
  privateCopyAgentsExpanded?: boolean;
  onPrivateCopyExpandedChange?: (expanded: boolean) => void;
}

export function AgentSelector({
  selectedAgents,
  privateCopyAgents = [],
  excludedPrivateCopyAgents = [],
  allAgents,
  onSelectionChange,
  onPrivateCopyChange,
  scope = 'global',
  privateCopyAgentsExpanded = false,
  onPrivateCopyExpandedChange,
}: AgentSelectorProps) {
  const { t } = useTranslation();
  const selectedAgentsRef = useRef(selectedAgents);
  const privateCopyAgentsRef = useRef(privateCopyAgents);
  const [isExpanded, setIsExpanded] = useState(false);

  useEffect(() => {
    selectedAgentsRef.current = selectedAgents;
  }, [selectedAgents]);
  useEffect(() => {
    privateCopyAgentsRef.current = privateCopyAgents;
  }, [privateCopyAgents]);

  const selectedAgentIds = useMemo(() => new Set(selectedAgents), [selectedAgents]);
  const privateCopyAgentIds = useMemo(() => new Set(privateCopyAgents), [privateCopyAgents]);
  const excludedPrivateCopyAgentIds = useMemo(
    () => new Set(excludedPrivateCopyAgents),
    [excludedPrivateCopyAgents]
  );

  const {
    detectedDefaultAvailable,
    undetectedDefaultAvailable,
    visiblePrivateRequiredAgents: detectedAgents,
    hiddenPrivateRequiredAgents: otherAgents,
    privateCopyEligibleAgents,
  } = useMemo(
    () => groupAgentsByScopedTarget(allAgents, scope, selectedAgentIds),
    [allAgents, scope, selectedAgentIds]
  );
  const visiblePrivateCopyEligibleAgents = useMemo(
    () => privateCopyEligibleAgents.filter((agent) => !excludedPrivateCopyAgentIds.has(agent.id)),
    [privateCopyEligibleAgents, excludedPrivateCopyAgentIds]
  );

  const toggleAgent = useCallback((agentId: string) => {
    const currentSelection = selectedAgentsRef.current;
    const isSelected = currentSelection.includes(agentId);
    const newSelection = isSelected
      ? currentSelection.filter((id) => id !== agentId)
      : [...currentSelection, agentId];
    selectedAgentsRef.current = newSelection;
    onSelectionChange(newSelection);
  }, [onSelectionChange]);

  const togglePrivateCopyAgent = useCallback((agentId: string) => {
    if (!onPrivateCopyChange) return;
    if (excludedPrivateCopyAgentIds.has(agentId)) return;

    const currentSelection = privateCopyAgentsRef.current;
    const isSelected = currentSelection.includes(agentId);
    const newSelection = isSelected
      ? currentSelection.filter((id) => id !== agentId)
      : [...currentSelection, agentId];
    privateCopyAgentsRef.current = newSelection;
    onPrivateCopyChange(newSelection);
  }, [onPrivateCopyChange, excludedPrivateCopyAgentIds]);

  const hasSelectableAgents = detectedAgents.length > 0 || otherAgents.length > 0;
  const hasDefaultAvailableAgents = detectedDefaultAvailable.length > 0 || undetectedDefaultAvailable.length > 0;
  const hasPrivateCopyOptions = visiblePrivateCopyEligibleAgents.length > 0 && Boolean(onPrivateCopyChange);

  return (
    <div className="space-y-5">
      {hasDefaultAvailableAgents && (
        <div className="space-y-3">
          <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
            <span className="text-sm font-semibold text-foreground tracking-tight">
              {t('addSkill.agents.defaultAvailableTitle')}
            </span>
            <span className="text-xs leading-5 text-muted-foreground">
              {t('addSkill.agents.defaultAvailableHint', { path: getSharedSkillDirectory(scope) })}
            </span>
          </div>

          <div className="space-y-3">
            {detectedDefaultAvailable.length > 0 ? (
              <div className="flex flex-wrap items-center gap-2">
                {detectedDefaultAvailable.map((agent) => (
                  <div key={agent.id} className="flex items-center gap-1.5 rounded border border-border/40 bg-muted/10 px-2 py-1">
                    <AgentIcon agentId={agent.id} className="h-4 w-4 bg-transparent border-0" iconClassName="h-3.5 w-3.5 text-foreground/80" />
                    <span className="text-xs font-medium text-foreground">{agent.name}</span>
                  </div>
                ))}
                {undetectedDefaultAvailable.length > 0 && (
                  <Tooltip delayDuration={200}>
                    <TooltipTrigger asChild>
                      <div className="cursor-help flex items-center justify-center rounded border border-dashed border-border/60 px-2 py-1 transition-colors hover:border-border hover:bg-muted/30">
                        <span className="text-[10px] text-muted-foreground">
                          {t('addSkill.agents.moreUndetected', { count: undetectedDefaultAvailable.length })}
                        </span>
                      </div>
                    </TooltipTrigger>
                    <TooltipContent side="top" showArrow={false} className="max-w-[280px] bg-popover text-popover-foreground border shadow-md p-2.5 space-y-1.5">
                      <div className="font-medium text-foreground text-xs">
                        {t('addSkill.agents.undetectedListPrefix')}
                      </div>
                      <div className="flex flex-wrap gap-1">
                        {undetectedDefaultAvailable.map(a => (
                          <span key={a.id} className="inline-flex items-center rounded-sm bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
                            {a.name}
                          </span>
                        ))}
                      </div>
                    </TooltipContent>
                  </Tooltip>
                )}
              </div>
            ) : (
              <div className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
                <span>{t('addSkill.agents.noDefaultAvailableAgents')}</span>
                {undetectedDefaultAvailable.length > 0 && (
                  <Tooltip delayDuration={200}>
                    <TooltipTrigger asChild>
                      <span className="cursor-help underline underline-offset-2 decoration-muted-foreground/40 hover:decoration-muted-foreground hover:text-foreground transition-colors">
                        {t('addSkill.agents.viewUndetectedInfo', { count: undetectedDefaultAvailable.length })}
                      </span>
                    </TooltipTrigger>
                    <TooltipContent side="top" showArrow={false} className="max-w-[280px] bg-popover text-popover-foreground border shadow-md p-2.5 space-y-1.5">
                      <div className="font-medium text-foreground text-xs">
                        {t('addSkill.agents.undetectedListPrefix')}
                      </div>
                      <div className="flex flex-wrap gap-1">
                        {undetectedDefaultAvailable.map(a => (
                          <span key={a.id} className="inline-flex items-center rounded-sm bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
                            {a.name}
                          </span>
                        ))}
                      </div>
                    </TooltipContent>
                  </Tooltip>
                )}
              </div>
            )}
          </div>
        </div>
      )}

      {hasSelectableAgents && (
        <div className="space-y-2">
          <div className="flex justify-between items-center pb-2 border-b border-border/40">
            <div className="flex items-center gap-2">
              <span className="text-sm font-semibold text-foreground">
                {t('addSkill.agents.privateRequiredTitle')}
              </span>
              <span className="text-[11px] text-muted-foreground truncate">
                {t('addSkill.agents.privateRequiredHint')}
              </span>
            </div>
          </div>

          <div className="flex flex-col space-y-1">
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
                <div className="relative py-2 flex items-center justify-center">
                  <div className="absolute inset-0 flex items-center" aria-hidden="true">
                    <div className="w-full border-t border-border/40" />
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="relative bg-background text-xs text-muted-foreground hover:bg-muted/50 hover:text-foreground transition-colors h-7 px-3"
                  >
                    {isExpanded ? (
                      <ChevronUp className="h-3.5 w-3.5" />
                    ) : (
                      <ChevronDown className="h-3.5 w-3.5" />
                    )}
                    {isExpanded
                      ? t('addSkill.agents.collapseOtherAgents')
                      : t('addSkill.agents.expandOtherAgents', { count: otherAgents.length })}
                  </Button>
                </div>
              </CollapsibleTrigger>
              <CollapsibleContent className="flex flex-col space-y-1 mt-1">
                {otherAgents.map((agent) => (
                  <AgentRow
                    key={agent.id}
                    agent={agent}
                    selected={selectedAgentIds.has(agent.id)}
                    onToggle={toggleAgent}
                    showDetectedBadge
                    scope={scope}
                    muted
                  />
                ))}
              </CollapsibleContent>
            </Collapsible>
          )}
        </div>
      )}

      {hasPrivateCopyOptions && (
        <Collapsible
          open={privateCopyAgentsExpanded || privateCopyAgents.length > 0}
          onOpenChange={onPrivateCopyExpandedChange}
        >
          <div className="rounded-md border border-border/50 bg-muted/10">
            <CollapsibleTrigger asChild>
              <button
                type="button"
                className="flex w-full items-center justify-between gap-3 px-3 py-2.5 text-left transition-colors hover:bg-muted/25"
              >
                <div className="min-w-0 space-y-0.5">
                  <div className="text-sm font-semibold text-foreground">
                    {t('addSkill.agents.privateCopyTitle')}
                  </div>
                  <div className="text-xs leading-5 text-muted-foreground">
                    {t('addSkill.agents.privateCopyHint')}
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-2 text-xs text-muted-foreground">
                  {privateCopyAgents.length > 0 && (
                    <span>{t('addSkill.agents.privateCopySelected', { count: privateCopyAgents.length })}</span>
                  )}
                  <ChevronDown className="h-3.5 w-3.5 transition-transform data-[state=open]:rotate-180" />
                </div>
              </button>
            </CollapsibleTrigger>
            <CollapsibleContent className="border-t border-border/40 px-2 py-2">
              <div className="flex flex-col space-y-1">
                {visiblePrivateCopyEligibleAgents.map((agent) => (
                  <AgentRow
                    key={agent.id}
                    agent={agent}
                    selected={privateCopyAgentIds.has(agent.id)}
                    onToggle={togglePrivateCopyAgent}
                    scope={scope}
                    pathOverride={getAgentTarget(agent, scope).privatePath ?? undefined}
                  />
                ))}
              </div>
            </CollapsibleContent>
          </div>
        </Collapsible>
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
  muted = false,
  pathOverride,
}: {
  agent: AgentInfo;
  selected: boolean;
  onToggle: (agentId: string) => void;
  showDetectedBadge?: boolean;
  scope?: 'global' | 'project';
  muted?: boolean;
  pathOverride?: string;
}) {
  const { t } = useTranslation();

  const target = scope ? getAgentTarget(agent, scope) : null;
  const path = pathOverride ?? target?.path;
  const isAbsolutePath = path ? /^[A-Za-z]:[\\/]|^\/|^\\\\/.test(path) : false;
  const pathLabel = path
    ? formatAgentTargetPath(scope === 'project' && !path.startsWith('./') && !isAbsolutePath ? `./${path}` : path)
    : undefined;

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={() => onToggle(agent.id)}
      onKeyDown={(event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          onToggle(agent.id);
        }
      }}
      className={cn(
        'group flex items-center justify-between gap-3 rounded-md px-3 py-2 outline-none transition-all duration-200 focus-visible:ring-2 focus-visible:ring-ring/35',
        selected ? 'bg-accent/40' : 'bg-transparent hover:bg-muted/30',
        muted && !selected ? 'opacity-60 grayscale-[0.5] hover:opacity-90' : 'opacity-100 grayscale-0'
      )}
    >
      <div className="flex items-center gap-3 min-w-0">
        <Checkbox
          checked={selected}
          className="pointer-events-none shrink-0"
        />
        <AgentIcon agentId={agent.id} className={cn("h-5 w-5 rounded-[4px] shrink-0", muted && !selected ? "opacity-70" : "")} />
        <span className={cn('truncate text-[13px] leading-none', selected ? 'font-medium text-foreground' : (muted ? 'text-muted-foreground' : 'font-medium text-foreground/90'))}>
          {agent.name}
        </span>
      </div>

      <div className="flex items-center gap-4 sm:gap-6 shrink-0 pl-2">
        {pathLabel && (
          <code className={cn("hidden sm:block font-mono text-[11px] leading-none", selected ? "text-muted-foreground" : "text-muted-foreground/60")}>
            {pathLabel}
          </code>
        )}

        {showDetectedBadge && (
          <span
            className={cn(
              'inline-flex shrink-0 items-center gap-1.5 rounded text-[11px] font-medium',
              agent.detected ? 'text-muted-foreground/80' : 'text-muted-foreground/50'
            )}
          >
            <span
              className={cn(
                'h-1.5 w-1.5 rounded-full',
                agent.detected ? 'bg-emerald-500/70' : 'bg-muted-foreground/30'
              )}
            />
            {agent.detected
              ? t('addSkill.agents.detected')
              : t('addSkill.agents.notDetected')}
          </span>
        )}
      </div>
    </div>
  );
});
