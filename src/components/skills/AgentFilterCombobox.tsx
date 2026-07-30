import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from 'react';
import { useTranslation } from 'react-i18next';
import { Check, ChevronDown, Filter, Search } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';
import { agentDisplayName, agentId } from '@/lib/agents';
import type { AgentId, ResolvedAgent } from '@/bindings';

interface AgentFilterComboboxProps {
  agents: ResolvedAgent[];
  selectedAgent: AgentId | null;
  onChange: (agentId: AgentId | null) => void;
  matchCounts: ReadonlyMap<AgentId, number>;
  totalSkillCount: number;
  compact?: boolean;
}

interface AgentFilterOption {
  id: AgentId | null;
  label: string;
  count: number;
}

export function AgentFilterCombobox({
  agents,
  selectedAgent,
  onChange,
  matchCounts,
  totalSkillCount,
  compact = false,
}: AgentFilterComboboxProps) {
  const { t } = useTranslation();
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const listboxId = useId();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [activeIndex, setActiveIndex] = useState(0);
  const [retainedAgentName, setRetainedAgentName] = useState<string>();

  const selected = agents.find((agent) => agentId(agent) === selectedAgent);
  const selectedAgentName = selected
    ? agentDisplayName(selected)
    : selectedAgent
      ? retainedAgentName
      : undefined;
  const normalizedQuery = query.trim().toLowerCase();
  const filteredAgents = useMemo(() => (
    normalizedQuery
      ? agents.filter((agent) => {
        const id = agentId(agent).toLowerCase();
        const name = agentDisplayName(agent).toLowerCase();
        return id.includes(normalizedQuery) || name.includes(normalizedQuery);
      })
      : agents
  ), [agents, normalizedQuery]);
  const options = useMemo<AgentFilterOption[]>(() => [
    ...(!normalizedQuery
      ? [{ id: null, label: t('skills.filter.allAgents'), count: totalSkillCount }]
      : []),
    ...filteredAgents.map((agent) => ({
      id: agentId(agent),
      label: agentDisplayName(agent),
      count: matchCounts.get(agentId(agent)) ?? 0,
    })),
  ], [filteredAgents, matchCounts, normalizedQuery, t, totalSkillCount]);

  useEffect(() => {
    if (!open) return;
    searchRef.current?.focus();

    const handlePointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener('pointerdown', handlePointerDown);
    return () => document.removeEventListener('pointerdown', handlePointerDown);
  }, [open]);

  const safeActiveIndex = Math.min(activeIndex, Math.max(options.length - 1, 0));

  const openList = () => {
    setQuery('');
    const selectedIndex = selectedAgent === null
      ? 0
      : agents.findIndex((agent) => agentId(agent) === selectedAgent) + 1;
    setActiveIndex(Math.max(selectedIndex, 0));
    setOpen(true);
  };

  const closeList = () => {
    setOpen(false);
    triggerRef.current?.focus();
  };

  const selectOption = (option: AgentFilterOption) => {
    setRetainedAgentName(option.id === null ? undefined : option.label);
    onChange(option.id);
    closeList();
  };

  const handleSearchKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      setActiveIndex((index) => options.length === 0 ? 0 : (index + 1) % options.length);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      setActiveIndex((index) => options.length === 0
        ? 0
        : (index - 1 + options.length) % options.length);
    } else if (event.key === 'Home') {
      event.preventDefault();
      setActiveIndex(0);
    } else if (event.key === 'End') {
      event.preventDefault();
      setActiveIndex(Math.max(options.length - 1, 0));
    } else if (event.key === 'Enter') {
      event.preventDefault();
      const option = options[safeActiveIndex];
      if (option) selectOption(option);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      closeList();
    } else if (event.key === 'Tab') {
      setOpen(false);
    }
  };

  return (
    <div
      ref={rootRef}
      className={cn('relative flex shrink-0 items-center', compact && 'w-full')}
    >
      <button
        ref={triggerRef}
        type="button"
        aria-label={t('skills.filter.agentLabel')}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={listboxId}
        className={cn(
          'flex h-8 min-w-0 items-center gap-1.5 rounded-md border border-transparent bg-muted px-2.5 text-sm text-foreground shadow-none outline-none transition-colors hover:bg-accent focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/50',
          selectedAgent !== null && 'bg-primary/10 text-primary hover:bg-primary/15',
          compact ? 'w-full' : 'min-w-[140px] max-w-56',
        )}
        onClick={() => open ? closeList() : openList()}
      >
        <Filter className="size-3.5 shrink-0" aria-hidden="true" />
        <span className="min-w-0 flex-1 truncate text-left">
          {selectedAgentName ?? selectedAgent ?? t('skills.filter.allAgents')}
        </span>
        <ChevronDown className="size-3.5 shrink-0 opacity-60" aria-hidden="true" />
      </button>

      {open && (
        <div className={cn(
          'absolute top-full z-50 mt-1 overflow-hidden rounded-md border border-border bg-popover text-popover-foreground shadow-md',
          compact ? 'inset-x-0' : 'right-0 w-64',
        )}>
          <div className="relative border-b border-border p-2">
            <Search className="pointer-events-none absolute left-4 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" aria-hidden="true" />
            <Input
              ref={searchRef}
              type="search"
              role="combobox"
              aria-label={t('skills.filter.searchAgents')}
              name="agent-filter-search"
              autoComplete="off"
              aria-autocomplete="list"
              aria-expanded="true"
              aria-controls={listboxId}
              aria-activedescendant={options[safeActiveIndex] ? `${listboxId}-option-${safeActiveIndex}` : undefined}
              placeholder={t('skills.filter.searchAgents')}
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
                setActiveIndex(0);
              }}
              onKeyDown={handleSearchKeyDown}
              className="h-8 pl-8 text-sm"
            />
          </div>
          <div id={listboxId} role="listbox" className="max-h-64 overflow-y-auto p-1">
            {options.length === 0 ? (
              <p className="px-3 py-5 text-center text-xs text-muted-foreground">
                {t('skills.filter.noMatchingAgents')}
              </p>
            ) : options.map((option, index) => {
              const selectedOption = option.id === selectedAgent;
              return (
                <button
                  key={option.id ?? 'all-agents'}
                  id={`${listboxId}-option-${index}`}
                  type="button"
                  role="option"
                  aria-selected={selectedOption}
                  aria-label={`${option.label} (${option.count})`}
                  tabIndex={-1}
                  className={cn(
                    'flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm outline-none',
                    safeActiveIndex === index && 'bg-accent text-accent-foreground',
                  )}
                  onMouseEnter={() => setActiveIndex(index)}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => selectOption(option)}
                >
                  <span className="flex size-4 shrink-0 items-center justify-center">
                    {selectedOption && <Check className="size-3.5" aria-hidden="true" />}
                  </span>
                  <span className="min-w-0 flex-1 truncate">{option.label}</span>
                  <span className="text-xs tabular-nums text-muted-foreground">{option.count}</span>
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
