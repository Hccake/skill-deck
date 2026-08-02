import { useMemo, useCallback, useEffect, useRef, memo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import { Button } from '@/components/ui/button';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { ChevronDown, ChevronUp, X } from 'lucide-react';
import {
  formatAgentTargetPath,
  groupAgentsByScopedTarget,
  getAgentDisplayPath,
  getAgentTarget,
  getSharedSkillDirectory,
  shouldDisplayAgentInitially,
} from '@/lib/agentTargets';
import { AgentIcon } from '@/components/agents/AgentIcon';
import { cn } from '@/lib/utils';
import { agentDisplayName, agentId, isAgentDetected } from '@/lib/agents';
import {
  buildAgentSelectionRows,
  isSelectionRowSelected,
  toggleSelectionRow,
  type AgentSelectionRow,
} from '@/lib/agentSelection';
import type { AgentId, AgentSelectionGroup, DetectionState, ResolvedAgent } from '@/bindings';

interface AgentSelectorProps {
  /** 选中的 agent IDs */
  selectedAgents: AgentId[];
  /** 额外保留到 Agent 目录中的可直接使用 Agent IDs */
  privateCopyAgents?: AgentId[];
  /** 所有 agents */
  allAgents: ResolvedAgent[];
  /** 当前 scope 下由 Backend 解析的独立目录分组 */
  selectionGroups?: AgentSelectionGroup[];
  /** 选择变化回调 */
  onSelectionChange: (agents: AgentId[]) => void;
  /** 额外保留选择变化回调 */
  onPrivateCopyChange?: (agents: AgentId[]) => void;
  /** 安装范围（用于动态显示自动应用路径） */
  scope?: 'global' | 'project';
  privateCopyAgentsExpanded?: boolean;
  onPrivateCopyExpandedChange?: (expanded: boolean) => void;
  unknownAgentIds?: AgentId[];
  onRemoveUnknownAgent?: (agentId: AgentId) => void;
  showPaths?: boolean;
}

export function AgentSelector({
  selectedAgents,
  privateCopyAgents = [],
  allAgents,
  selectionGroups = [],
  onSelectionChange,
  onPrivateCopyChange,
  scope = 'global',
  privateCopyAgentsExpanded = false,
  onPrivateCopyExpandedChange,
  unknownAgentIds = [],
  onRemoveUnknownAgent,
  showPaths = true,
}: AgentSelectorProps) {
  const { t } = useTranslation();
  const selectedAgentsRef = useRef(selectedAgents);
  const keptAgentDirectoryAgentsRef = useRef(privateCopyAgents);
  const [isExpanded, setIsExpanded] = useState(false);
  const [keptAgentDirectoryUndetectedExpanded, setKeptAgentDirectoryUndetectedExpanded] = useState(false);

  useEffect(() => {
    selectedAgentsRef.current = selectedAgents;
  }, [selectedAgents]);
  useEffect(() => {
    keptAgentDirectoryAgentsRef.current = privateCopyAgents;
  }, [privateCopyAgents]);

  const selectedAgentIds = useMemo(() => new Set(selectedAgents), [selectedAgents]);
  const keptAgentDirectoryAgentIds = useMemo(() => new Set(privateCopyAgents), [privateCopyAgents]);
  const {
    visibleDefaultAvailableAgents,
    hiddenDefaultAvailableAgents,
    visiblePrivateRequiredAgents,
    hiddenPrivateRequiredAgents,
    privateCopyEligibleAgents,
  } = useMemo(
    () => groupAgentsByScopedTarget(allAgents, scope, selectedAgentIds),
    [allAgents, scope, selectedAgentIds]
  );
  const requiredRows = useMemo(
    () => buildAgentSelectionRows(
      allAgents,
      selectionGroups,
      [...visiblePrivateRequiredAgents, ...hiddenPrivateRequiredAgents],
    ),
    [allAgents, hiddenPrivateRequiredAgents, selectionGroups, visiblePrivateRequiredAgents],
  );
  const requiredGroupIds = useMemo(
    () => new Set(requiredRows.map((row) => row.groupId)),
    [requiredRows],
  );
  const privateCopyRows = useMemo(
    () => buildAgentSelectionRows(allAgents, selectionGroups, privateCopyEligibleAgents)
      .filter((row) => !requiredGroupIds.has(row.groupId)),
    [allAgents, privateCopyEligibleAgents, requiredGroupIds, selectionGroups],
  );
  const { visibleRows: visibleRequiredRows, hiddenRows: hiddenRequiredRows } = useMemo(
    () => splitSelectionRows(requiredRows, selectedAgentIds),
    [requiredRows, selectedAgentIds],
  );
  const {
    visibleRows: visibleKeptAgentDirectoryRows,
    hiddenRows: hiddenKeptAgentDirectoryRows,
  } = useMemo(
    () => splitSelectionRows(privateCopyRows, keptAgentDirectoryAgentIds),
    [keptAgentDirectoryAgentIds, privateCopyRows],
  );

  const toggleAgent = useCallback((toggledIds: AgentId[]) => {
    const currentSelection = selectedAgentsRef.current;
    const newSelection = toggleSelectionRow(currentSelection, toggledIds);
    selectedAgentsRef.current = newSelection;
    onSelectionChange(newSelection);
  }, [onSelectionChange]);

  const togglePrivateCopyAgent = useCallback((toggledIds: AgentId[]) => {
    if (!onPrivateCopyChange) return;

    const currentSelection = keptAgentDirectoryAgentsRef.current;
    const newSelection = toggleSelectionRow(currentSelection, toggledIds);
    keptAgentDirectoryAgentsRef.current = newSelection;
    onPrivateCopyChange(newSelection);
  }, [onPrivateCopyChange]);

  const hasSelectableAgents = requiredRows.length > 0;
  const hasDefaultAvailableAgents = visibleDefaultAvailableAgents.length > 0
    || hiddenDefaultAvailableAgents.length > 0;
  const hasPrivateCopyOptions = privateCopyRows.length > 0 && Boolean(onPrivateCopyChange);

  return (
    <div className="min-w-0 max-w-full space-y-5">
      {unknownAgentIds.length > 0 ? (
        <div className="space-y-2">
          <div className="space-y-0.5">
            <p className="text-sm font-semibold">{t('addSkill.agents.unknownTitle')}</p>
            <p className="text-xs text-muted-foreground">
              {t(onRemoveUnknownAgent
                ? 'addSkill.agents.unknownRemovableHint'
                : 'addSkill.agents.unknownHint')}
            </p>
          </div>
          {unknownAgentIds.map((unknownId) => (
            <div
              key={unknownId}
              className="flex min-w-0 items-center gap-3 rounded-md border border-warning/40 bg-warning/5 px-3 py-2"
            >
              <AgentIcon agentId={unknownId} className="h-8 w-8" />
              <span className="min-w-0 flex-1 truncate text-sm font-medium">{unknownId}</span>
              <span className="rounded-full border border-warning/40 bg-background/70 px-2 py-0.5 text-[11px] font-medium text-warning">
                {t('addSkill.agents.unknownUnavailable')}
              </span>
              {onRemoveUnknownAgent ? (
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-sm"
                  aria-label={t('addSkill.agents.removeUnknown')}
                  title={t('addSkill.agents.removeUnknown')}
                  onClick={() => onRemoveUnknownAgent(unknownId)}
                >
                  <X className="h-3.5 w-3.5" />
                </Button>
              ) : null}
            </div>
          ))}
        </div>
      ) : null}
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
            {visibleDefaultAvailableAgents.length > 0 ? (
              <div className="flex flex-wrap items-center gap-2">
                {visibleDefaultAvailableAgents.map((agent) => (
                  <div
                    key={agentId(agent)}
                    className={cn(
                      'flex items-center gap-1.5 rounded border border-border/40 bg-muted/10 px-2 py-1',
                      !isAgentDetected(agent) && 'opacity-70',
                    )}
                  >
                    <AgentIcon agentId={agentId(agent)} className="h-4 w-4 bg-transparent border-0" iconClassName="h-3.5 w-3.5 text-foreground/80" />
                    <span className="text-xs font-medium text-foreground">{agentDisplayName(agent)}</span>
                  </div>
                ))}
                {hiddenDefaultAvailableAgents.length > 0 && (
                  <Tooltip delayDuration={200}>
                    <TooltipTrigger asChild>
                      <div className="cursor-help flex items-center justify-center rounded border border-dashed border-border/60 px-2 py-1 transition-colors hover:border-border hover:bg-muted/30">
                        <span className="text-[10px] text-muted-foreground">
                          {t('addSkill.agents.moreUndetected', { count: hiddenDefaultAvailableAgents.length })}
                        </span>
                      </div>
                    </TooltipTrigger>
                    <TooltipContent side="top" showArrow={false} className="max-w-[280px] bg-popover text-popover-foreground border shadow-md p-2.5 space-y-1.5">
                      <div className="font-medium text-foreground text-xs">
                        {t('addSkill.agents.undetectedListPrefix')}
                      </div>
                      <div className="flex flex-wrap gap-1">
                        {hiddenDefaultAvailableAgents.map(a => (
                          <span key={agentId(a)} className="inline-flex items-center rounded-sm bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
                            {agentDisplayName(a)}
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
                {hiddenDefaultAvailableAgents.length > 0 && (
                  <Tooltip delayDuration={200}>
                    <TooltipTrigger asChild>
                      <span className="cursor-help underline underline-offset-2 decoration-muted-foreground/40 hover:decoration-muted-foreground hover:text-foreground transition-colors">
                        {t('addSkill.agents.viewUndetectedInfo', { count: hiddenDefaultAvailableAgents.length })}
                      </span>
                    </TooltipTrigger>
                    <TooltipContent side="top" showArrow={false} className="max-w-[280px] bg-popover text-popover-foreground border shadow-md p-2.5 space-y-1.5">
                      <div className="font-medium text-foreground text-xs">
                        {t('addSkill.agents.undetectedListPrefix')}
                      </div>
                      <div className="flex flex-wrap gap-1">
                        {hiddenDefaultAvailableAgents.map(a => (
                          <span key={agentId(a)} className="inline-flex items-center rounded-sm bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
                            {agentDisplayName(a)}
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
          <div className="flex min-w-0 justify-between items-center pb-2 border-b border-border/40">
            <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
              <span className="shrink-0 text-sm font-semibold text-foreground">
                {t('addSkill.agents.privateRequiredTitle')}
              </span>
              <span className="min-w-0 text-[11px] text-muted-foreground">
                {t('addSkill.agents.privateRequiredHint')}
              </span>
            </div>
          </div>

          <div className="flex flex-col space-y-1">
            {visibleRequiredRows.map((row) => (
              <AgentRow
                key={row.groupId}
                row={row}
                selected={isSelectionRowSelected(row, selectedAgentIds)}
                onToggle={toggleAgent}
                showDetectedBadge
                scope={scope}
                showPath={showPaths}
              />
            ))}
          </div>

          {hiddenRequiredRows.length > 0 && (
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
                      : t('addSkill.agents.expandOtherAgents', { count: hiddenRequiredRows.length })}
                  </Button>
                </div>
              </CollapsibleTrigger>
              <CollapsibleContent className="flex flex-col space-y-1 mt-1">
                {hiddenRequiredRows.map((row) => (
                  <AgentRow
                    key={row.groupId}
                    row={row}
                    selected={isSelectionRowSelected(row, selectedAgentIds)}
                    onToggle={toggleAgent}
                    showDetectedBadge
                    scope={scope}
                    showPath={showPaths}
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
          <div className="space-y-1">
            <CollapsibleTrigger asChild>
              <button
                type="button"
                className="flex w-full items-center justify-between gap-3 rounded-md px-1 py-1.5 text-left transition-colors hover:bg-muted/25"
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
            <CollapsibleContent className="pt-1">
              <div className="flex flex-col space-y-1">
                {visibleKeptAgentDirectoryRows.map((row) => (
                  <AgentRow
                    key={row.groupId}
                    row={row}
                    selected={isSelectionRowSelected(row, keptAgentDirectoryAgentIds)}
                    onToggle={togglePrivateCopyAgent}
                    scope={scope}
                    pathOverride={row.agents.length === 1
                      ? getAgentTarget(row.agents[0], scope).privatePath ?? undefined
                      : undefined}
                    showPath={showPaths}
                  />
                ))}
                {hiddenKeptAgentDirectoryRows.length > 0 && (
                  <Collapsible open={keptAgentDirectoryUndetectedExpanded} onOpenChange={setKeptAgentDirectoryUndetectedExpanded}>
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
                          {keptAgentDirectoryUndetectedExpanded ? (
                            <ChevronUp className="h-3.5 w-3.5" />
                          ) : (
                            <ChevronDown className="h-3.5 w-3.5" />
                          )}
                          {keptAgentDirectoryUndetectedExpanded
                            ? t('addSkill.agents.collapseOtherAgents')
                            : t('addSkill.agents.expandOtherAgents', { count: hiddenKeptAgentDirectoryRows.length })}
                        </Button>
                      </div>
                    </CollapsibleTrigger>
                    <CollapsibleContent className="flex flex-col space-y-1 mt-1">
                      {hiddenKeptAgentDirectoryRows.map((row) => (
                        <AgentRow
                          key={row.groupId}
                          row={row}
                          selected={isSelectionRowSelected(row, keptAgentDirectoryAgentIds)}
                          onToggle={togglePrivateCopyAgent}
                          scope={scope}
                          pathOverride={row.agents.length === 1
                            ? getAgentTarget(row.agents[0], scope).privatePath ?? undefined
                            : undefined}
                          showPath={showPaths}
                          muted
                        />
                      ))}
                    </CollapsibleContent>
                  </Collapsible>
                )}
              </div>
            </CollapsibleContent>
          </div>
        </Collapsible>
      )}
    </div>
  );
}

const AgentRow = memo(function AgentRow({
  row,
  selected,
  onToggle,
  showDetectedBadge = false,
  scope,
  muted = false,
  pathOverride,
  showPath = true,
}: {
  row: AgentSelectionRow;
  selected: boolean;
  onToggle: (agentIds: AgentId[]) => void;
  showDetectedBadge?: boolean;
  scope?: 'global' | 'project';
  muted?: boolean;
  pathOverride?: string;
  showPath?: boolean;
}) {
  const { t } = useTranslation();
  const singleAgent = row.agents.length === 1 ? row.agents[0] : null;
  const displayName = row.agents.map(agentDisplayName).join(' / ');

  const path = showPath && singleAgent
    ? pathOverride ?? (scope ? getAgentDisplayPath(singleAgent, scope) : null)
    : null;
  const isAbsolutePath = path ? /^[A-Za-z]:[\\/]|^\/|^\\\\/.test(path) : false;
  const pathLabel = path
    ? formatAgentTargetPath(scope === 'project' && !path.startsWith('./') && !isAbsolutePath ? `./${path}` : path)
    : undefined;
  const checkboxId = `agent-selector-${scope ?? 'unknown'}-${row.groupId.replace(/[^a-zA-Z0-9_-]/g, '-')}`;

  return (
    <Label
      htmlFor={checkboxId}
      className={cn(
        'group flex cursor-pointer items-center justify-between gap-3 rounded-md px-3 py-2 transition-all duration-200',
        selected ? 'bg-accent/40' : 'bg-transparent hover:bg-muted/30',
        muted && !selected ? 'opacity-60 grayscale-[0.5] hover:opacity-90' : 'opacity-100 grayscale-0'
      )}
    >
      <div className="flex min-w-0 flex-1 items-center gap-3">
        <Checkbox
          id={checkboxId}
          checked={selected}
          onCheckedChange={() => onToggle(row.selectableAgentIds)}
          className="shrink-0"
        />
        <span className="flex shrink-0 items-center -space-x-1">
          {row.agents.slice(0, 3).map((agent) => (
            <AgentIcon
              key={agentId(agent)}
              agentId={agentId(agent)}
              className={cn("h-5 w-5 rounded-[4px] ring-1 ring-background", muted && !selected ? "opacity-70" : "")}
            />
          ))}
        </span>
        <span className={cn('truncate text-[13px] leading-none', selected ? 'font-medium text-foreground' : (muted ? 'text-muted-foreground' : 'font-medium text-foreground/90'))}>
          {displayName}
        </span>
      </div>

      <div className="flex min-w-0 flex-1 items-center justify-end gap-3 pl-2 sm:gap-4">
        {pathLabel && (
          <code className={cn("hidden min-w-0 truncate sm:block font-mono text-[11px] leading-none", selected ? "text-muted-foreground" : "text-muted-foreground/60")}>
            {pathLabel}
          </code>
        )}

        {showDetectedBadge && singleAgent && (
          <span
            className={cn(
              'inline-flex shrink-0 items-center gap-1.5 rounded text-[11px] font-medium',
              detectionTextClass(singleAgent.detection),
            )}
          >
            <span
              className={cn(
                'h-1.5 w-1.5 rounded-full',
                detectionDotClass(singleAgent.detection),
              )}
            />
            {t(`addSkill.agents.${singleAgent.detection}`)}
          </span>
        )}
      </div>
    </Label>
  );
});

function splitSelectionRows(
  rows: AgentSelectionRow[],
  selectedAgentIds: ReadonlySet<AgentId>,
): { visibleRows: AgentSelectionRow[]; hiddenRows: AgentSelectionRow[] } {
  const visibleRows: AgentSelectionRow[] = [];
  const hiddenRows: AgentSelectionRow[] = [];
  for (const row of rows) {
    const visible = isSelectionRowSelected(row, selectedAgentIds)
      || row.agents.some((agent) => shouldDisplayAgentInitially(agent, false));
    (visible ? visibleRows : hiddenRows).push(row);
  }
  return { visibleRows, hiddenRows };
}

function detectionTextClass(detection: DetectionState): string {
  if (detection === 'detected') return 'text-muted-foreground/80';
  if (detection === 'indeterminate') return 'text-amber-700/80 dark:text-amber-300/80';
  return 'text-muted-foreground/50';
}

function detectionDotClass(detection: DetectionState): string {
  if (detection === 'detected') return 'bg-emerald-500/70';
  if (detection === 'indeterminate') return 'bg-amber-500/70';
  return 'bg-muted-foreground/30';
}
