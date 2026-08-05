import { Bot, ChevronDown, Info, TriangleAlert, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type {
  AgentSelectionDisplayGroup,
  AgentSelectionItem,
  AgentSelectionItemId,
  AgentSelectionSnapshot,
  InstallMode,
  ManageSelectionItemState,
} from '@/bindings';
import { Checkbox } from '@/components/ui/checkbox';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import {
  Popover,
  PopoverClose,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import {
  groupSelectionState,
  type AgentSelectionSession,
} from '@/lib/agent-selection-session';

interface AgentSelectionViewProps {
  snapshot: AgentSelectionSnapshot;
  session: AgentSelectionSession;
  itemStates?: ManageSelectionItemState[];
  onItemChange: (itemId: AgentSelectionItemId, selected: boolean) => void;
  onGroupChange: (groupId: string, selected: boolean) => void;
  onOtherExpandedChange: (expanded: boolean) => void;
  onAdditionalExpandedChange: (expanded: boolean) => void;
  onGroupExpandedChange: (groupId: string, expanded: boolean) => void;
  disabled?: boolean;
  emptyMessage?: string;
}

export function AgentSelectionView({
  snapshot,
  session,
  itemStates = [],
  onItemChange,
  onGroupChange,
  onOtherExpandedChange,
  onAdditionalExpandedChange,
  onGroupExpandedChange,
  disabled = false,
  emptyMessage,
}: AgentSelectionViewProps) {
  const { t, i18n } = useTranslation();
  const selected = new Set(session.selectedItemIds);
  const states = new Map(itemStates.map((state) => [state.itemId, state]));
  const agents = new Map(snapshot.agents.map((agent) => [agent.id, agent]));
  const separate = snapshot.items.filter((item) => item.category === 'separateInstall' && !item.groupId);
  const detected = separate.filter((item) => item.agentIds.some((id) => agents.get(id)?.detection === 'detected'));
  const other = separate.filter((item) => !detected.includes(item));
  const additional = snapshot.items.filter((item) => item.category === 'additionalInstall');
  const hasSelectionContent = snapshot.directAgentIds.length > 0 || snapshot.items.length > 0;
  const unknownAgentNames = snapshot.unavailableExplicitAgents.map((item) => item.agentId);
  const unknownAgents = new Intl.ListFormat(
    i18n?.resolvedLanguage ?? i18n?.language,
    { style: 'long', type: 'conjunction' },
  ).format(unknownAgentNames);

  return (
    <div className="space-y-5">
      {snapshot.unavailableExplicitAgents.length > 0 ? (
        <div className="flex gap-2 rounded-md bg-muted/60 px-3 py-2.5 text-sm text-muted-foreground" role="status">
          <Info className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
          <span>{t('agentSelection.unknownNotice', { agents: unknownAgents })}</span>
        </div>
      ) : null}

      {!hasSelectionContent && emptyMessage ? (
        <p className="py-8 text-center text-sm text-muted-foreground">{emptyMessage}</p>
      ) : null}

      <DirectAgentsSummary snapshot={snapshot} />

      {(detected.length > 0 || other.length > 0 || snapshot.groups.length > 0) ? (
        <section aria-labelledby="separate-install-title" className="space-y-2">
          <h3 id="separate-install-title" className="text-sm font-semibold">
            {t('agentSelection.separateInstall')}
          </h3>
          <div className="space-y-1">
            {detected.map((item) => (
              <SelectionRow key={item.id} {...rowProps(item)} />
            ))}
            {snapshot.groups.map((group) => (
              <SelectionGroup key={group.id} group={group} />
            ))}
            {other.length > 0 ? (
              <Collapsible open={session.otherAgentsExpanded} onOpenChange={onOtherExpandedChange}>
                <CollapsibleTrigger className="flex h-9 w-full items-center justify-between rounded px-2 text-sm text-muted-foreground hover:bg-muted/60">
                  <span>{t('agentSelection.otherAgents', { count: other.length })}</span>
                  <ChevronDown className={cn('size-4 transition-transform', session.otherAgentsExpanded && 'rotate-180')} aria-hidden="true" />
                </CollapsibleTrigger>
                <CollapsibleContent className="space-y-1 pt-1">
                  {other.map((item) => <SelectionRow key={item.id} {...rowProps(item)} />)}
                </CollapsibleContent>
              </Collapsible>
            ) : null}
          </div>
        </section>
      ) : null}

      {additional.length > 0 ? (
        <Collapsible open={session.additionalInstallExpanded} onOpenChange={onAdditionalExpandedChange}>
          <section aria-labelledby="additional-install-title" className="space-y-2">
            <CollapsibleTrigger className="flex w-full items-start justify-between gap-3 text-left">
              <span className="space-y-0.5">
                <span id="additional-install-title" className="block text-sm font-semibold">
                  {t('agentSelection.additionalInstall')}
                </span>
                <span className="block text-xs leading-5 text-muted-foreground">
                  {t('agentSelection.additionalInstallDescription')}
                </span>
              </span>
              <ChevronDown className={cn('mt-0.5 size-4 shrink-0 text-muted-foreground transition-transform', session.additionalInstallExpanded && 'rotate-180')} aria-hidden="true" />
            </CollapsibleTrigger>
            <CollapsibleContent className="space-y-1">
              {additional.map((item) => <SelectionRow key={item.id} {...rowProps(item)} />)}
            </CollapsibleContent>
          </section>
        </Collapsible>
      ) : null}
    </div>
  );

  function rowProps(item: AgentSelectionItem) {
    return {
      item,
      snapshot,
      state: states.get(item.id),
      selected: selected.has(item.id),
      mode: session.mode,
      disabled,
      onChange: onItemChange,
    };
  }

  function SelectionGroup({ group }: { group: AgentSelectionDisplayGroup }) {
    const expanded = session.expandedGroupIds.includes(group.id);
    const groupItems = group.itemIds.flatMap((id) => snapshot.items.filter((item) => item.id === id));
    const changeableItems = groupItems.filter((item) => {
      const state = states.get(item.id);
      return item.selectable && (!state || state.allowedResults === 'both');
    });
    const checked = groupSelectionState(session, snapshot, group.id);
    return (
      <Collapsible open={expanded} onOpenChange={(open) => onGroupExpandedChange(group.id, open)}>
        <div className="grid min-h-10 grid-cols-[1rem_1.25rem_minmax(0,1fr)_auto_auto] items-center gap-2 rounded-md px-2 hover:bg-muted/50">
          <Checkbox
            checked={checked}
            onCheckedChange={(value) => {
              const nextSelected = value === true;
              if (changeableItems.length === groupItems.length) {
                onGroupChange(group.id, nextSelected);
              } else {
                changeableItems.forEach((item) => onItemChange(item.id, nextSelected));
              }
            }}
            disabled={disabled || changeableItems.length === 0}
            aria-label={group.displayName}
          />
          <Bot className="size-4 text-muted-foreground" aria-hidden="true" />
          <span className="truncate text-sm font-medium">{group.displayName}</span>
          <DetectionText value={group.detection} />
          <CollapsibleTrigger className="grid size-7 place-items-center rounded hover:bg-muted" aria-label={t('agentSelection.toggleGroup', { agent: group.displayName })}>
            <ChevronDown className={cn('size-4 transition-transform', expanded && 'rotate-180')} aria-hidden="true" />
          </CollapsibleTrigger>
        </div>
        <CollapsibleContent className="ml-7 space-y-1 border-l pl-2">
          {groupItems.map((item) => <SelectionRow key={item.id} {...rowProps(item)} compact />)}
        </CollapsibleContent>
      </Collapsible>
    );
  }
}

function DirectAgentsSummary({ snapshot }: { snapshot: AgentSelectionSnapshot }) {
  const { t } = useTranslation();
  const direct = snapshot.directAgentIds.flatMap((id) => snapshot.agents.filter((agent) => agent.id === id));
  if (direct.length === 0) return null;
  const detected = direct.filter((agent) => agent.detection === 'detected');
  const more = direct.filter((agent) => agent.detection !== 'detected');
  return (
    <section aria-labelledby="direct-agents-title" className="space-y-2">
      <h3 id="direct-agents-title" className="text-sm font-semibold">{t('agentSelection.directUse')}</h3>
      <div className="flex flex-wrap items-center gap-1.5">
        {detected.map((agent) => (
          <span key={agent.id} className="inline-flex h-7 items-center gap-1.5 rounded-md bg-muted px-2 text-xs">
            <Bot className="size-3.5 text-muted-foreground" aria-hidden="true" />
            {agent.displayName}
          </span>
        ))}
        {more.length > 0 ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <span tabIndex={0} className="inline-flex h-7 items-center rounded-md px-2 text-xs text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring">
                {t('agentSelection.moreAgents', { count: more.length })}
              </span>
            </TooltipTrigger>
            <TooltipContent className="max-w-80 space-y-2" sideOffset={6}>
              <p>{t('agentSelection.moreAgentsDescription')}</p>
              <div className="flex flex-wrap gap-x-3 gap-y-1 text-background/80">
                {more.map((agent) => <span key={agent.id}>{agent.displayName}</span>)}
              </div>
            </TooltipContent>
          </Tooltip>
        ) : null}
      </div>
    </section>
  );
}

function SelectionRow({
  item,
  snapshot,
  state,
  selected,
  mode,
  disabled,
  compact = false,
  onChange,
}: {
  item: AgentSelectionItem;
  snapshot: AgentSelectionSnapshot;
  state?: ManageSelectionItemState;
  selected: boolean;
  mode: InstallMode;
  disabled: boolean;
  compact?: boolean;
  onChange: (itemId: AgentSelectionItemId, selected: boolean) => void;
}) {
  const { t } = useTranslation();
  const members = item.agentIds.flatMap((id) => snapshot.agents.filter((agent) => agent.id === id));
  const detected = members.filter((agent) => agent.detection === 'detected').length;
  const lockedSelected = state?.allowedResults === 'selected';
  const readOnly = !item.selectable || (state !== undefined && state.allowedResults !== 'both');
  const status = state ? currentEntryText(state.currentEntry, t) : null;
  const effect = state ? effectText(item, state, selected, mode, t) : null;
  const disabledReason = item.disabledReason
    ? t(`agentSelection.disabled.${item.disabledReason}`)
    : null;
  return (
    <div className={cn('grid min-h-10 grid-cols-[1rem_1.25rem_minmax(0,1fr)_auto_auto] items-center gap-2 rounded-md px-2', !readOnly && 'hover:bg-muted/50')}>
      {lockedSelected ? (
        <Checkbox checked disabled aria-label={item.displayName} />
      ) : readOnly ? <TriangleAlert className="size-4 text-warning" aria-hidden="true" /> : (
        <Checkbox
          checked={selected}
          onCheckedChange={(value) => onChange(item.id, value === true)}
          disabled={disabled}
          aria-label={item.displayName}
        />
      )}
      <Bot className="size-4 text-muted-foreground" aria-hidden="true" />
      <span className="flex min-w-0 items-center gap-1.5">
        <Tooltip>
          <TooltipTrigger asChild>
            <span tabIndex={0} className={cn('min-w-0 truncate text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring', compact && 'text-[13px]')}>
              {item.displayName}
            </span>
          </TooltipTrigger>
          <TooltipContent sideOffset={6}><span translate="no">{item.path}</span></TooltipContent>
        </Tooltip>
        {members.length > 1 ? (
          <>
            <span className="shrink-0 text-xs text-muted-foreground">
              {t('agentSelection.memberCount', { count: members.length })}
            </span>
            <Popover>
              <PopoverTrigger asChild>
                <button type="button" className="shrink-0 text-xs text-muted-foreground underline-offset-2 hover:text-foreground hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" aria-label={t('agentSelection.viewMembers')}>
                  {t('agentSelection.view')}
                </button>
              </PopoverTrigger>
              <PopoverContent
                className="w-72 space-y-1.5"
                align="start"
                sideOffset={6}
                aria-label={t('agentSelection.viewMembers')}
              >
                <PopoverClose className="absolute right-2 top-2 grid size-7 place-items-center rounded text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" aria-label={t('common.close')}>
                  <X className="size-4" aria-hidden="true" />
                </PopoverClose>
                {members.map((member) => (
                  <span key={member.id} className="flex items-center justify-between gap-4">
                    <span>{member.displayName}</span>
                    <span className="text-muted-foreground">{t(`agentSelection.detection.${member.detection}`)}</span>
                  </span>
                ))}
              </PopoverContent>
            </Popover>
          </>
        ) : null}
      </span>
      <span className="flex items-center gap-2 whitespace-nowrap text-xs">
        {disabledReason ? <span className="text-warning">{disabledReason}</span> : null}
        {status ? <span className={cn((state?.currentEntry === 'brokenLink' || state?.currentEntry === 'unrecognized') && 'text-warning')}>{status}</span> : null}
        {effect ? <span className="text-muted-foreground">{effect}</span> : null}
        {item.modeConstraint === 'copyOnly' && !state ? <span className="text-muted-foreground">{t('agentSelection.copy')}</span> : null}
      </span>
      <span className="min-w-16 text-right text-xs text-muted-foreground">
        {members.length > 1 ? t('agentSelection.detectedCount', { detected, total: members.length }) : <DetectionText value={members[0]?.detection ?? 'indeterminate'} />}
      </span>
    </div>
  );
}

function DetectionText({ value }: { value: 'detected' | 'notDetected' | 'indeterminate' }) {
  const { t } = useTranslation();
  return <span className="text-xs text-muted-foreground">{t(`agentSelection.detection.${value}`)}</span>;
}

function currentEntryText(value: ManageSelectionItemState['currentEntry'], t: (key: string) => string) {
  if (value === 'none') return null;
  return t(`agentSelection.current.${value}`);
}

function effectText(
  item: AgentSelectionItem,
  state: ManageSelectionItemState,
  selected: boolean,
  mode: InstallMode,
  t: (key: string) => string,
) {
  if (!selected && state.unselectedEffect === 'remove') return t('agentSelection.effect.remove');
  if (!selected) return null;
  if (state.selectedEffect === 'repair') {
    return mode === 'copy' && item.modeConstraint === 'userSelectable'
      ? t('agentSelection.effect.createCopy')
      : t('agentSelection.effect.repair');
  }
  if (state.selectedEffect === 'add') {
    if (item.modeConstraint === 'copyOnly') return t('agentSelection.effect.copy');
    return t(mode === 'copy' ? 'agentSelection.effect.createCopy' : 'agentSelection.effect.createLink');
  }
  return null;
}
