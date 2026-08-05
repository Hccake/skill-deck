import { useEffect, useId, useRef, useState } from 'react';
import {
  Bot,
  ChevronDown,
  ChevronRight,
  CircleHelp,
  Copy,
  Info,
  Link2,
  TriangleAlert,
  UsersRound,
  X,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type {
  AgentInstallOption,
  AgentInstallOptionId,
  AgentSelectionGroup,
  AgentSelectionSnapshot,
  DetectionState,
  InstallMode,
  ManageInstallOptionState,
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
import { projectAgentSelectionView } from '@/lib/agent-selection-view';
import type { AgentSelectionPresentation } from './useAgentSelectionPresentation';

const ROW_GRID = 'grid-cols-[1rem_minmax(0,1fr)_minmax(7.5rem,auto)_6rem]';
const DIRECT_AGENT_BADGE = 'inline-flex h-7 items-center gap-1.5 rounded-md border border-border/70 bg-muted/30 px-2 text-xs font-medium';

interface AgentSelectionViewProps {
  presentation: AgentSelectionPresentation;
  snapshot: AgentSelectionSnapshot;
  session: AgentSelectionSession;
  optionStates?: ManageInstallOptionState[];
  onOptionChange: (optionId: AgentInstallOptionId, selected: boolean) => void;
  onGroupChange: (groupId: string, selected: boolean) => void;
  onOtherExpandedChange: (expanded: boolean) => void;
  onAdditionalExpandedChange: (expanded: boolean) => void;
  onGroupExpandedChange: (groupId: string, expanded: boolean) => void;
  disabled?: boolean;
  emptyMessage?: string;
}

export function AgentSelectionView({
  presentation,
  snapshot,
  session,
  optionStates = [],
  onOptionChange,
  onGroupChange,
  onOtherExpandedChange,
  onAdditionalExpandedChange,
  onGroupExpandedChange,
  disabled = false,
  emptyMessage,
}: AgentSelectionViewProps) {
  const { t } = useTranslation();
  const states = new Map(optionStates.map((state) => [state.optionId, state]));
  const { agentsById, directAgents, separateOptions, additionalOptions } = projectAgentSelectionView(snapshot);
  const detected = separateOptions.filter((option) => option.agentIds.some((id) => agentsById.get(id)?.detection === 'detected'));
  const other = separateOptions.filter((option) => !detected.includes(option));
  const hasSelectionContent = directAgents.length > 0 || snapshot.installOptions.length > 0;
  const commonRowProps = { snapshot, session, states, disabled, onOptionChange };

  return (
    <div className="space-y-6">
      {!hasSelectionContent && emptyMessage ? (
        <p className="py-8 text-center text-sm text-muted-foreground">{emptyMessage}</p>
      ) : null}

      <DirectAgentsSummary
        agents={directAgents}
        options={additionalOptions}
        presentation={presentation}
        {...commonRowProps}
        onExpandedChange={onAdditionalExpandedChange}
      />

      {(detected.length > 0 || other.length > 0 || snapshot.groups.length > 0) ? (
        <section aria-labelledby="separate-install-title" className="space-y-2">
          <SectionHeading
            id="separate-install-title"
            title={presentation.selectable.title}
            help={presentation.selectable.help}
          />
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

function DirectAgentsSummary({
  agents,
  options,
  presentation,
  snapshot,
  session,
  states,
  disabled,
  onOptionChange,
  onExpandedChange,
}: CommonRowProps & {
  agents: AgentSelectionSnapshot['agents'];
  options: AgentInstallOption[];
  presentation: AgentSelectionPresentation;
  onExpandedChange: (expanded: boolean) => void;
}) {
  if (agents.length === 0) return null;
  const detected = agents.filter((agent) => agent.detection === 'detected');
  const more = agents.filter((agent) => agent.detection !== 'detected');
  const agentsById = new Map(agents.map((agent) => [agent.id, agent]));
  const orderedOptions = [
    ...options.filter((option) => option.agentIds.some((id) => agentsById.get(id)?.detection === 'detected')),
    ...options.filter((option) => !option.agentIds.some((id) => agentsById.get(id)?.detection === 'detected')),
  ];
  const selectedIds = new Set(session.selectedOptionIds);
  const selectedAgentCount = new Set(
    options
      .filter((option) => selectedIds.has(option.id))
      .flatMap((option) => option.agentIds),
  ).size;
  return (
    <section aria-labelledby="direct-agents-title" className="space-y-2">
      <SectionHeading
        id="direct-agents-title"
        title={presentation.automatic.title}
        help={presentation.automatic.help}
      />
      <div className="flex flex-wrap items-center gap-2 px-1">
        {detected.map((agent) => (
          <DirectAgentBadge key={agent.id} name={agent.displayName} />
        ))}
        {more.length > 0 ? <DirectAgentsMorePopover agents={more} /> : null}
      </div>
      {orderedOptions.length > 0 ? (
        <Collapsible
          open={session.additionalInstallExpanded}
          onOpenChange={onExpandedChange}
          className="pt-1"
        >
          <CollapsibleTrigger
            className="flex min-h-9 w-full items-center gap-2 rounded-md px-1 text-left text-sm font-medium text-muted-foreground hover:bg-muted/60 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            <ChevronRight
              className={cn(
                'size-4 shrink-0 transition-transform',
                session.additionalInstallExpanded && 'rotate-90',
              )}
              aria-hidden="true"
            />
            <span>{presentation.ownDirectory.title}</span>
            {selectedAgentCount > 0 ? (
              <span className="ml-auto pr-1 text-xs font-normal text-muted-foreground">
                {presentation.ownDirectory.selectedCount(selectedAgentCount)}
              </span>
            ) : null}
          </CollapsibleTrigger>
          <CollapsibleContent className="pt-1">
            <p className="px-1 pl-7 text-xs leading-5 text-muted-foreground">
              {presentation.ownDirectory.description}
            </p>
            <div className="mt-2 space-y-1 pl-5">
              {orderedOptions.map((item) => (
                <SelectionRow
                  key={item.id}
                  item={item}
                  snapshot={snapshot}
                  session={session}
                  states={states}
                  disabled={disabled}
                  onOptionChange={onOptionChange}
                />
              ))}
            </div>
          </CollapsibleContent>
        </Collapsible>
      ) : null}
    </section>
  );
}

function SectionHeading({ id, title, help }: { id: string; title: string; help: string }) {
  return (
    <div className="flex items-center gap-1 px-1">
      <h3 id={id} className="text-sm font-semibold">{title}</h3>
      <Tooltip>
        <TooltipTrigger asChild>
          <button
            type="button"
            className="inline-flex size-6 shrink-0 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            aria-label={help}
          >
            <CircleHelp className="size-3.5" aria-hidden="true" />
          </button>
        </TooltipTrigger>
        <TooltipContent className="max-w-72 text-xs leading-5">
          {help}
        </TooltipContent>
      </Tooltip>
    </div>
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
  states: Map<AgentInstallOptionId, ManageInstallOptionState>;
  disabled: boolean;
  onOptionChange: (optionId: AgentInstallOptionId, selected: boolean) => void;
}

function SelectionGroup({
  group,
  snapshot,
  session,
  states,
  disabled,
  onOptionChange,
  onGroupChange,
  onGroupExpandedChange,
}: CommonRowProps & {
  group: AgentSelectionGroup;
  onGroupChange: (groupId: string, selected: boolean) => void;
  onGroupExpandedChange: (groupId: string, expanded: boolean) => void;
}) {
  const { t } = useTranslation();
  const checkboxId = useId();
  const expanded = session.expandedGroupIds.includes(group.id);
  const groupOptions = group.optionIds.flatMap((id) => snapshot.installOptions.filter((option) => option.id === id));
  const changeableOptions = groupOptions.filter((option) => {
    const state = states.get(option.id);
    return option.selectable && (!state || state.allowedResults === 'both');
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
            if (changeableOptions.length === groupOptions.length) {
              onGroupChange(group.id, nextSelected);
            } else {
              changeableOptions.forEach((option) => onOptionChange(option.id, nextSelected));
            }
          }}
          disabled={disabled || changeableOptions.length === 0}
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
        {groupOptions.map((option) => (
          <SelectionGroupChild
            key={option.id}
            option={option}
            snapshot={snapshot}
            session={session}
            states={states}
            disabled={disabled}
            onOptionChange={onOptionChange}
          />
        ))}
      </CollapsibleContent>
    </Collapsible>
  );
}

function SelectionGroupChild({ option, session, states, disabled, onOptionChange }: CommonRowProps & { option: AgentInstallOption }) {
  const checkboxId = useId();
  const selected = session.selectedOptionIds.includes(option.id);
  const state = states.get(option.id);
  const readOnly = !option.selectable || (state !== undefined && state.allowedResults !== 'both');
  return (
    <div data-slot="agent-selection-group-child" className={cn('grid min-h-10 items-center gap-2 rounded-md px-2', ROW_GRID, !readOnly && 'hover:bg-muted/50')}>
      <span aria-hidden="true" />
      <span className="flex min-w-0 items-center gap-2 pl-7">
        <SelectionCheckbox
          id={checkboxId}
          option={option}
          state={state}
          selected={selected}
          disabled={disabled}
          onOptionChange={onOptionChange}
        />
        <PathLabel id={readOnly ? undefined : checkboxId} option={option} className="text-[13px]" />
      </span>
      <EntryState option={option} state={state} selected={selected} mode={session.mode} />
      <span aria-hidden="true" />
    </div>
  );
}

function SelectionRow({ item: option, snapshot, session, states, disabled, onOptionChange }: CommonRowProps & { item: AgentInstallOption }) {
  const { t, i18n } = useTranslation();
  const checkboxId = useId();
  const state = states.get(option.id);
  const selected = session.selectedOptionIds.includes(option.id);
  const members = option.agentIds.flatMap((id) => snapshot.agents.filter((agent) => agent.id === id));
  const detected = members.filter((agent) => agent.detection === 'detected').length;
  const readOnly = !option.selectable || (state !== undefined && state.allowedResults !== 'both');
  const displayName = members.length > 1
    ? mergedAgentNames(members, i18n?.resolvedLanguage ?? i18n?.language, t)
    : option.displayName;
  return (
    <div data-slot="agent-selection-row" className={cn('grid min-h-11 items-center gap-2 rounded-md px-2', ROW_GRID, !readOnly && 'hover:bg-muted/50')}>
      <SelectionCheckbox
        id={checkboxId}
        option={option}
        state={state}
        selected={selected}
        disabled={disabled}
        label={displayName}
        onOptionChange={onOptionChange}
      />
      <span className="flex min-w-0 items-center gap-2">
        <PathLabel
          id={readOnly ? undefined : checkboxId}
          option={option}
          label={displayName}
          showGlyph
          glyph={members.length > 1 ? UsersRound : undefined}
          glyphSlot={members.length > 1 ? 'agent-group-glyph' : undefined}
        />
        {members.length > 1 ? <MembersPopover members={members} /> : null}
      </span>
      <EntryState option={option} state={state} selected={selected} mode={session.mode} />
      <span className="justify-self-end text-right text-xs text-muted-foreground">
        {members.length > 1
          ? <DetectedCount members={members} detected={detected} />
          : <DetectionText value={members[0]?.detection ?? 'indeterminate'} />}
      </span>
    </div>
  );
}

function SelectionCheckbox({ id, option, state, selected, disabled, label, onOptionChange }: {
  id: string;
  option: AgentInstallOption;
  state?: ManageInstallOptionState;
  selected: boolean;
  disabled: boolean;
  label?: string;
  onOptionChange: (optionId: AgentInstallOptionId, selected: boolean) => void;
}) {
  const accessibleLabel = label ?? option.displayName;
  const lockedSelected = state?.allowedResults === 'selected';
  const readOnly = !option.selectable || (state !== undefined && state.allowedResults !== 'both');
  if (lockedSelected) return <Checkbox id={id} checked disabled aria-label={accessibleLabel} />;
  if (readOnly) return <TriangleAlert className="size-4 text-warning" aria-hidden="true" />;
  return (
    <Checkbox
      id={id}
      checked={selected}
      onCheckedChange={(value) => onOptionChange(option.id, value === true)}
      disabled={disabled}
      aria-label={accessibleLabel}
    />
  );
}

function PathLabel({ id, option, label, className, showGlyph = false, glyph, glyphSlot }: {
  id?: string;
  option: AgentInstallOption;
  label?: string;
  className?: string;
  showGlyph?: boolean;
  glyph?: typeof Bot;
  glyphSlot?: string;
}) {
  const content = (
    <>
      {showGlyph ? <AgentGlyph icon={glyph} slot={glyphSlot} /> : null}
      <span className="min-w-0 truncate">{label ?? option.displayName}</span>
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
      <TooltipContent sideOffset={6}><code translate="no">{option.path}</code></TooltipContent>
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

function EntryState({ option, state, selected, mode }: {
  option: AgentInstallOption;
  state?: ManageInstallOptionState;
  selected: boolean;
  mode: InstallMode;
}) {
  const { t } = useTranslation();
  const current = state ? currentEntryPresentation(state.currentEntry, t) : null;
  const effect = state ? effectPresentation(option, state, selected, mode, t) : null;
  const disabledReason = option.disabledReason ? t(`agentSelection.disabled.${option.disabledReason}`) : null;
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
      {option.modeConstraint === 'copyOnly' && !state ? (
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

function currentEntryPresentation(value: ManageInstallOptionState['currentEntry'], t: (key: string) => string) {
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
  option: AgentInstallOption,
  state: ManageInstallOptionState,
  selected: boolean,
  mode: InstallMode,
  t: (key: string) => string,
) {
  if (!selected && state.unselectedEffect === 'remove') {
    return { label: t('agentSelection.effect.remove'), tone: 'destructive' as const };
  }
  if (!selected) return null;
  if (state.selectedEffect === 'repair') {
    return mode === 'copy' && option.modeConstraint === 'userSelectable'
      ? { label: t('agentSelection.effect.createCopy'), tone: 'default' as const }
      : { label: t('agentSelection.effect.repair'), tone: 'warning' as const };
  }
  if (state.selectedEffect === 'add') {
    if (option.modeConstraint === 'copyOnly') {
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
