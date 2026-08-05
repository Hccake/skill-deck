import { useEffect, useId, useRef, useState } from 'react';
import {
  Bot,
  ChevronDown,
  Copy,
  Info,
  Link2,
  TriangleAlert,
  UsersRound,
  X,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type {
  AgentSelectionDisplayGroup,
  AgentSelectionItem,
  AgentSelectionItemId,
  AgentSelectionSnapshot,
  DetectionState,
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

const ROW_GRID = 'grid-cols-[1rem_minmax(0,1fr)_minmax(7.5rem,auto)_6rem]';
const DIRECT_AGENT_BADGE = 'inline-flex h-7 items-center gap-1.5 rounded-md border border-border/70 bg-muted/30 px-2 text-xs font-medium';

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
  const { t } = useTranslation();
  const states = new Map(itemStates.map((state) => [state.itemId, state]));
  const agents = new Map(snapshot.agents.map((agent) => [agent.id, agent]));
  const separate = snapshot.items.filter((item) => item.category === 'separateInstall' && !item.groupId);
  const detected = separate.filter((item) => item.agentIds.some((id) => agents.get(id)?.detection === 'detected'));
  const other = separate.filter((item) => !detected.includes(item));
  const additional = snapshot.items.filter((item) => item.category === 'additionalInstall');
  const hasSelectionContent = snapshot.directAgentIds.length > 0 || snapshot.items.length > 0;
  const commonRowProps = { snapshot, session, states, disabled, onItemChange };

  return (
    <div className="space-y-6">
      {!hasSelectionContent && emptyMessage ? (
        <p className="py-8 text-center text-sm text-muted-foreground">{emptyMessage}</p>
      ) : null}

      <DirectAgentsSummary snapshot={snapshot} />

      {(detected.length > 0 || other.length > 0 || snapshot.groups.length > 0) ? (
        <section aria-labelledby="separate-install-title" className="space-y-2">
          <h3 id="separate-install-title" className="px-1 text-sm font-semibold">
            {t('agentSelection.separateInstall')}
          </h3>
          <div className="space-y-1">
            {detected.map((item) => (
              <SelectionRow key={item.id} item={item} {...commonRowProps} />
            ))}
            {snapshot.groups.map((group) => (
              <SelectionGroup
                key={group.id}
                group={group}
                {...commonRowProps}
                onGroupChange={onGroupChange}
                onGroupExpandedChange={onGroupExpandedChange}
              />
            ))}
            {other.length > 0 ? (
              <Collapsible open={session.otherAgentsExpanded} onOpenChange={onOtherExpandedChange}>
                <CollapsibleTrigger className="flex h-9 w-full items-center justify-center gap-1.5 rounded px-2 text-sm text-muted-foreground hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
                  <span>{t('agentSelection.otherAgents', { count: other.length })}</span>
                  <ChevronDown className={cn('size-4 transition-transform', session.otherAgentsExpanded && 'rotate-180')} aria-hidden="true" />
                </CollapsibleTrigger>
                <CollapsibleContent className="space-y-1 pt-1">
                  {other.map((item) => <SelectionRow key={item.id} item={item} {...commonRowProps} />)}
                </CollapsibleContent>
              </Collapsible>
            ) : null}
          </div>
        </section>
      ) : null}

      {additional.length > 0 ? (
        <Collapsible open={session.additionalInstallExpanded} onOpenChange={onAdditionalExpandedChange}>
          <section aria-labelledby="additional-install-title" className="space-y-2">
            <CollapsibleTrigger className="flex w-full items-start justify-between gap-3 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
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
              {additional.map((item) => <SelectionRow key={item.id} item={item} {...commonRowProps} />)}
            </CollapsibleContent>
          </section>
        </Collapsible>
      ) : null}
    </div>
  );
}

export function AgentSelectionUnavailableNotice({ snapshot }: { snapshot: AgentSelectionSnapshot }) {
  const { t, i18n } = useTranslation();
  if (snapshot.unavailableExplicitAgents.length === 0) return null;
  const names = snapshot.unavailableExplicitAgents.map((item) => item.agentId);
  const agents = new Intl.ListFormat(
    i18n?.resolvedLanguage ?? i18n?.language,
    { style: 'long', type: 'conjunction' },
  ).format(names);
  return (
    <div className="flex gap-2 rounded-md bg-muted/60 px-3 py-2.5 text-sm text-muted-foreground" role="status">
      <Info className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
      <span>{t('agentSelection.unknownNotice', { agents })}</span>
    </div>
  );
}

function DirectAgentsSummary({ snapshot }: { snapshot: AgentSelectionSnapshot }) {
  const { t } = useTranslation();
  const direct = snapshot.directAgentIds.flatMap((id) => snapshot.agents.filter((agent) => agent.id === id));
  if (direct.length === 0) return null;
  const detected = direct.filter((agent) => agent.detection === 'detected');
  const more = direct.filter((agent) => agent.detection !== 'detected');
  return (
    <section aria-labelledby="direct-agents-title" className="space-y-2">
      <h3 id="direct-agents-title" className="px-1 text-sm font-semibold">{t('agentSelection.directUse')}</h3>
      <div className="flex flex-wrap items-center gap-2 px-1">
        {detected.map((agent) => (
          <DirectAgentBadge key={agent.id} name={agent.displayName} />
        ))}
        {more.length > 0 ? <DirectAgentsMorePopover agents={more} /> : null}
      </div>
    </section>
  );
}

function DirectAgentsMorePopover({ agents }: { agents: AgentSelectionSnapshot['agents'] }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const cancelClose = () => {
    if (closeTimer.current !== null) {
      clearTimeout(closeTimer.current);
      closeTimer.current = null;
    }
  };

  const show = () => {
    cancelClose();
    setOpen(true);
  };

  const scheduleClose = () => {
    cancelClose();
    closeTimer.current = setTimeout(() => setOpen(false), 100);
  };

  useEffect(() => cancelClose, []);

  const label = t('agentSelection.moreAgents', { count: agents.length });
  return (
    <Popover open={open} onOpenChange={(nextOpen) => {
      if (!nextOpen) setOpen(false);
    }}>
      <PopoverTrigger asChild>
        <button
          type="button"
          data-slot="direct-agent-badge"
          className={cn(
            DIRECT_AGENT_BADGE,
            'text-muted-foreground hover:bg-muted/60 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
          )}
          onPointerEnter={show}
          onPointerLeave={scheduleClose}
          onFocus={show}
          onBlur={scheduleClose}
          onClick={(event) => event.preventDefault()}
        >
          {label}
        </button>
      </PopoverTrigger>
      <PopoverContent
        className="w-max max-w-[calc(100vw-2rem)] space-y-2.5"
        align="start"
        sideOffset={6}
        aria-label={label}
        onPointerEnter={cancelClose}
        onPointerLeave={scheduleClose}
        onFocusCapture={show}
        onBlurCapture={scheduleClose}
        onOpenAutoFocus={(event) => event.preventDefault()}
        onCloseAutoFocus={(event) => event.preventDefault()}
      >
        <p className="whitespace-nowrap text-xs text-muted-foreground">
          {t('agentSelection.moreAgentsDescription')}
        </p>
        <div className="flex max-w-96 flex-wrap gap-2">
          {agents.map((agent) => (
            <DirectAgentBadge key={agent.id} name={agent.displayName} />
          ))}
        </div>
      </PopoverContent>
    </Popover>
  );
}

function DirectAgentBadge({ name }: { name: string }) {
  return (
    <span data-slot="direct-agent-badge" className={cn(DIRECT_AGENT_BADGE, 'text-foreground')}>
      <Bot className="size-3.5 text-muted-foreground" aria-hidden="true" />
      {name}
    </span>
  );
}

interface CommonRowProps {
  snapshot: AgentSelectionSnapshot;
  session: AgentSelectionSession;
  states: Map<AgentSelectionItemId, ManageSelectionItemState>;
  disabled: boolean;
  onItemChange: (itemId: AgentSelectionItemId, selected: boolean) => void;
}

function SelectionGroup({
  group,
  snapshot,
  session,
  states,
  disabled,
  onItemChange,
  onGroupChange,
  onGroupExpandedChange,
}: CommonRowProps & {
  group: AgentSelectionDisplayGroup;
  onGroupChange: (groupId: string, selected: boolean) => void;
  onGroupExpandedChange: (groupId: string, expanded: boolean) => void;
}) {
  const { t } = useTranslation();
  const checkboxId = useId();
  const expanded = session.expandedGroupIds.includes(group.id);
  const groupItems = group.itemIds.flatMap((id) => snapshot.items.filter((item) => item.id === id));
  const changeableItems = groupItems.filter((item) => {
    const state = states.get(item.id);
    return item.selectable && (!state || state.allowedResults === 'both');
  });
  const checked = groupSelectionState(session, snapshot, group.id);
  return (
    <Collapsible
      open={expanded}
      onOpenChange={(open) => onGroupExpandedChange(group.id, open)}
      role="group"
      aria-label={group.displayName}
    >
      <div className={cn('grid min-h-11 items-center gap-2 rounded-md px-2 hover:bg-muted/50', ROW_GRID)}>
        <Checkbox
          id={checkboxId}
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
        <span className="flex min-w-0 items-center gap-2">
          <label htmlFor={checkboxId} className="flex min-w-0 flex-1 cursor-pointer items-center gap-2">
            <AgentGlyph />
            <span className="min-w-0 truncate text-sm font-medium">{group.displayName}</span>
          </label>
          <CollapsibleTrigger className="grid size-7 shrink-0 place-items-center rounded hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" aria-label={t('agentSelection.toggleGroup', { agent: group.displayName })}>
            <ChevronDown className={cn('size-4 transition-transform', expanded && 'rotate-180')} aria-hidden="true" />
          </CollapsibleTrigger>
        </span>
        <span className="inline-flex items-center gap-1 text-xs text-muted-foreground">
          <Copy className="size-3.5" aria-hidden="true" />
          {t('agentSelection.copyOnly')}
        </span>
        <DetectionText value={group.detection} className="justify-self-end text-right" />
      </div>
      <CollapsibleContent className="space-y-1">
        {groupItems.map((item) => (
          <SelectionGroupChild
            key={item.id}
            item={item}
            snapshot={snapshot}
            session={session}
            states={states}
            disabled={disabled}
            onItemChange={onItemChange}
          />
        ))}
      </CollapsibleContent>
    </Collapsible>
  );
}

function SelectionGroupChild({ item, session, states, disabled, onItemChange }: CommonRowProps & { item: AgentSelectionItem }) {
  const checkboxId = useId();
  const selected = session.selectedItemIds.includes(item.id);
  const state = states.get(item.id);
  const readOnly = !item.selectable || (state !== undefined && state.allowedResults !== 'both');
  return (
    <div data-slot="agent-selection-group-child" className={cn('grid min-h-10 items-center gap-2 rounded-md px-2', ROW_GRID, !readOnly && 'hover:bg-muted/50')}>
      <span aria-hidden="true" />
      <span className="flex min-w-0 items-center gap-2 pl-7">
        <SelectionCheckbox
          id={checkboxId}
          item={item}
          state={state}
          selected={selected}
          disabled={disabled}
          onItemChange={onItemChange}
        />
        <PathLabel id={readOnly ? undefined : checkboxId} item={item} className="text-[13px]" />
      </span>
      <EntryState item={item} state={state} selected={selected} mode={session.mode} />
      <span aria-hidden="true" />
    </div>
  );
}

function SelectionRow({ item, snapshot, session, states, disabled, onItemChange }: CommonRowProps & { item: AgentSelectionItem }) {
  const { t, i18n } = useTranslation();
  const checkboxId = useId();
  const state = states.get(item.id);
  const selected = session.selectedItemIds.includes(item.id);
  const members = item.agentIds.flatMap((id) => snapshot.agents.filter((agent) => agent.id === id));
  const detected = members.filter((agent) => agent.detection === 'detected').length;
  const readOnly = !item.selectable || (state !== undefined && state.allowedResults !== 'both');
  const displayName = members.length > 1
    ? mergedAgentNames(members, i18n?.resolvedLanguage ?? i18n?.language, t)
    : item.displayName;
  return (
    <div data-slot="agent-selection-row" className={cn('grid min-h-11 items-center gap-2 rounded-md px-2', ROW_GRID, !readOnly && 'hover:bg-muted/50')}>
      <SelectionCheckbox
        id={checkboxId}
        item={item}
        state={state}
        selected={selected}
        disabled={disabled}
        label={displayName}
        onItemChange={onItemChange}
      />
      <span className="flex min-w-0 items-center gap-2">
        <PathLabel
          id={readOnly ? undefined : checkboxId}
          item={item}
          label={displayName}
          showGlyph
          glyph={members.length > 1 ? UsersRound : undefined}
          glyphSlot={members.length > 1 ? 'agent-group-glyph' : undefined}
        />
        {members.length > 1 ? <MembersPopover members={members} /> : null}
      </span>
      <EntryState item={item} state={state} selected={selected} mode={session.mode} />
      <span className="justify-self-end text-right text-xs text-muted-foreground">
        {members.length > 1
          ? <DetectedCount members={members} detected={detected} />
          : <DetectionText value={members[0]?.detection ?? 'indeterminate'} />}
      </span>
    </div>
  );
}

function SelectionCheckbox({ id, item, state, selected, disabled, label, onItemChange }: {
  id: string;
  item: AgentSelectionItem;
  state?: ManageSelectionItemState;
  selected: boolean;
  disabled: boolean;
  label?: string;
  onItemChange: (itemId: AgentSelectionItemId, selected: boolean) => void;
}) {
  const accessibleLabel = label ?? item.displayName;
  const lockedSelected = state?.allowedResults === 'selected';
  const readOnly = !item.selectable || (state !== undefined && state.allowedResults !== 'both');
  if (lockedSelected) return <Checkbox id={id} checked disabled aria-label={accessibleLabel} />;
  if (readOnly) return <TriangleAlert className="size-4 text-warning" aria-hidden="true" />;
  return (
    <Checkbox
      id={id}
      checked={selected}
      onCheckedChange={(value) => onItemChange(item.id, value === true)}
      disabled={disabled}
      aria-label={accessibleLabel}
    />
  );
}

function PathLabel({ id, item, label, className, showGlyph = false, glyph, glyphSlot }: {
  id?: string;
  item: AgentSelectionItem;
  label?: string;
  className?: string;
  showGlyph?: boolean;
  glyph?: typeof Bot;
  glyphSlot?: string;
}) {
  const content = (
    <>
      {showGlyph ? <AgentGlyph icon={glyph} slot={glyphSlot} /> : null}
      <span className="min-w-0 truncate">{label ?? item.displayName}</span>
    </>
  );
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        {id ? (
          <label htmlFor={id} tabIndex={0} className={cn('flex min-w-0 flex-1 items-center gap-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring', 'cursor-pointer', className)}>
            {content}
          </label>
        ) : (
          <span tabIndex={0} className={cn('flex min-w-0 flex-1 items-center gap-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring', className)}>
            {content}
          </span>
        )}
      </TooltipTrigger>
      <TooltipContent sideOffset={6}><code translate="no">{item.path}</code></TooltipContent>
    </Tooltip>
  );
}

function MembersPopover({ members }: { members: AgentSelectionSnapshot['agents'] }) {
  const { t } = useTranslation();
  return (
    <span className="flex shrink-0 items-center">
      <Popover>
        <PopoverTrigger asChild>
          <button type="button" className="shrink-0 text-xs text-muted-foreground underline-offset-2 hover:text-foreground hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" aria-label={t('agentSelection.viewMembers')}>
            {t('agentSelection.memberCount', { count: members.length })}
          </button>
        </PopoverTrigger>
        <PopoverContent className="w-72 space-y-1.5" align="start" sideOffset={6} aria-label={t('agentSelection.viewMembers')}>
          <PopoverClose className="absolute right-2 top-2 grid size-7 place-items-center rounded text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" aria-label={t('common.close')}>
            <X className="size-4" aria-hidden="true" />
          </PopoverClose>
          <p className="pr-8 text-xs leading-5 text-muted-foreground">
            {t('agentSelection.sharedPlacementDescription')}
          </p>
          {members.map((member) => (
            <span key={member.id} className="flex items-center justify-between gap-4">
              <span>{member.displayName}</span>
              <DetectionText value={member.detection} />
            </span>
          ))}
        </PopoverContent>
      </Popover>
    </span>
  );
}

function EntryState({ item, state, selected, mode }: {
  item: AgentSelectionItem;
  state?: ManageSelectionItemState;
  selected: boolean;
  mode: InstallMode;
}) {
  const { t } = useTranslation();
  const current = state ? currentEntryPresentation(state.currentEntry, t) : null;
  const effect = state ? effectPresentation(item, state, selected, mode, t) : null;
  const disabledReason = item.disabledReason ? t(`agentSelection.disabled.${item.disabledReason}`) : null;
  return (
    <span data-slot="agent-entry-state" className="flex min-w-0 items-center gap-2 whitespace-nowrap text-xs">
      {disabledReason ? <span className="text-warning">{disabledReason}</span> : null}
      {current ? (
        <span className={cn('inline-flex items-center gap-1 text-muted-foreground', current.warning && 'text-warning')}>
          <current.Icon className="size-3.5" aria-hidden="true" />
          {current.label}
        </span>
      ) : null}
      {current && effect ? <span className="text-muted-foreground" aria-hidden="true">→</span> : null}
      {effect ? (
        <span className={cn(
          effect.tone === 'destructive' && 'text-destructive',
          effect.tone === 'warning' && 'text-warning',
          effect.tone === 'default' && 'text-foreground',
        )}>
          {effect.label}
        </span>
      ) : null}
      {item.modeConstraint === 'copyOnly' && !state ? (
        <span className="inline-flex items-center gap-1 text-muted-foreground">
          <Copy className="size-3.5" aria-hidden="true" />
          {t('agentSelection.copy')}
        </span>
      ) : null}
    </span>
  );
}

function AgentGlyph({ small = false, icon: Icon = Bot, slot }: {
  small?: boolean;
  icon?: typeof Bot;
  slot?: string;
}) {
  return (
    <span data-slot={slot} className={cn('grid shrink-0 place-items-center rounded border bg-muted/60 text-muted-foreground', small ? 'size-5' : 'size-7')} aria-hidden="true">
      <Icon className={small ? 'size-3' : 'size-4'} />
    </span>
  );
}

function DetectionText({ value, className }: { value: DetectionState; className?: string }) {
  const { t } = useTranslation();
  return (
    <span className={cn('inline-flex items-center gap-1.5 text-xs text-muted-foreground', className)}>
      <DetectionDot
        tone={value === 'detected' ? 'detected' : value === 'indeterminate' ? 'warning' : 'neutral'}
      />
      {t(`agentSelection.detection.${value}`)}
    </span>
  );
}

function DetectedCount({ members, detected }: {
  members: AgentSelectionSnapshot['agents'];
  detected: number;
}) {
  const { t } = useTranslation();
  const hasIndeterminate = members.some((member) => member.detection === 'indeterminate');
  const total = members.length;
  return (
    <span className="inline-flex items-center gap-1.5">
      <DetectionDot
        tone={hasIndeterminate ? 'warning' : detected === total ? 'detected' : 'neutral'}
      />
      {t('agentSelection.detectedCount', { detected, total })}
    </span>
  );
}

function DetectionDot({ tone }: { tone: 'detected' | 'neutral' | 'warning' }) {
  return (
    <span
      data-slot="agent-detection-dot"
      className={cn(
        'size-1.5 shrink-0 rounded-full',
        tone === 'detected' && 'bg-emerald-500 dark:bg-emerald-400',
        tone === 'neutral' && 'bg-muted-foreground/60',
        tone === 'warning' && 'bg-warning',
      )}
      aria-hidden="true"
    />
  );
}

function currentEntryPresentation(value: ManageSelectionItemState['currentEntry'], t: (key: string) => string) {
  if (value === 'none') return null;
  if (value === 'copy') return { label: t('agentSelection.current.copy'), Icon: Copy, warning: false };
  if (value === 'link') return { label: t('agentSelection.current.link'), Icon: Link2, warning: false };
  return {
    label: t(`agentSelection.current.${value}`),
    Icon: TriangleAlert,
    warning: true,
  };
}

function effectPresentation(
  item: AgentSelectionItem,
  state: ManageSelectionItemState,
  selected: boolean,
  mode: InstallMode,
  t: (key: string) => string,
) {
  if (!selected && state.unselectedEffect === 'remove') {
    return { label: t('agentSelection.effect.remove'), tone: 'destructive' as const };
  }
  if (!selected) return null;
  if (state.selectedEffect === 'repair') {
    return mode === 'copy' && item.modeConstraint === 'userSelectable'
      ? { label: t('agentSelection.effect.createCopy'), tone: 'default' as const }
      : { label: t('agentSelection.effect.repair'), tone: 'warning' as const };
  }
  if (state.selectedEffect === 'add') {
    if (item.modeConstraint === 'copyOnly') {
      return { label: t('agentSelection.effect.copy'), tone: 'default' as const };
    }
    return {
      label: t(mode === 'copy' ? 'agentSelection.effect.createCopy' : 'agentSelection.effect.createLink'),
      tone: 'default' as const,
    };
  }
  return null;
}

function mergedAgentNames(
  members: AgentSelectionSnapshot['agents'],
  language: string | undefined,
  t: (key: string, values?: Record<string, unknown>) => string,
) {
  const visibleNames = members.slice(0, 2).map((member) => member.displayName);
  const names = new Intl.ListFormat(language, { style: 'short', type: 'conjunction' }).format(visibleNames);
  return members.length > 2
    ? t('agentSelection.mergedAgentNamesMore', { names })
    : names;
}
