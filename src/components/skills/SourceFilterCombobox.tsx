import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, ChevronDown, Folder, Globe, Package, CircleHelp, Search } from 'lucide-react';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { OverflowTooltip } from '@/components/ui/overflow-tooltip';
import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';
import type { SourceFilterOption } from '@/lib/skills/filter';
import type { TFunction } from 'i18next';

function sourceLabel(source: SourceFilterOption, t: TFunction) {
  if (source.kind === 'local') return t('skills.updateStatusLabel.localSource');
  if (source.kind === 'incomplete') return t('skills.filter.incompleteSource');
  if (source.kind === 'dependency') return source.label
    ? t('skills.filter.dependencyPackage', { name: source.label }) : t('skills.filter.dependencySource');
  return source.label;
}

interface Props {
  options: SourceFilterOption[];
  selected: SourceFilterOption | null;
  onChange: (source: SourceFilterOption | null) => void;
  totalCount: number;
  compact?: boolean;
}

export function SourceFilterCombobox({ options, selected, onChange, totalCount, compact = false }: Props) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [activeIndex, setActiveIndex] = useState(0);
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const listId = useId();
  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return options.filter(option => `${sourceLabel(option, t)} ${option.detail}`.toLowerCase().includes(needle));
  }, [options, query, t]);
  const groups = useMemo(() => {
    const groups = new Map<string, SourceFilterOption[]>();
    for (const option of options) {
      const key = `${option.kind}\0${option.label}`;
      const group = groups.get(key);
      if (group) group.push(option); else groups.set(key, [option]);
    }
    return groups;
  }, [options]);
  const choices = [null, ...filtered];
  const active = Math.min(activeIndex, choices.length - 1);
  useEffect(() => {
    if (open) listRef.current?.querySelectorAll('[role="option"]')[active]?.scrollIntoView?.({ block: 'nearest' });
  }, [active, open]);

  const select = (option: SourceFilterOption | null) => {
    onChange(option);
    setOpen(false);
  };
  const selectedLabel = selected ? sourceLabel(selected, t) : t(compact ? 'skills.filter.sourceShort' : 'skills.filter.allSources');
  return (
    <Popover open={open} onOpenChange={next => {
      if (next) { setQuery(''); setActiveIndex(selected ? Math.max(0, options.findIndex(option => option.key === selected.key) + 1) : 0); }
      setOpen(next);
    }}>
      <OverflowTooltip content={selected ? `${sourceLabel(selected, t)} ${selected.detail}` : t('skills.filter.allSources')}>
        <PopoverTrigger asChild>
          <button type="button" aria-label={t('skills.filter.sourceLabel')} aria-haspopup="listbox" aria-expanded={open} aria-controls={listId}
            className={cn('flex h-8 min-w-0 items-center gap-1.5 rounded-md border border-transparent bg-muted px-2 text-sm text-foreground shadow-none outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring/50', compact ? 'w-full text-xs' : 'max-w-48', selected && 'bg-primary/10 text-primary hover:bg-primary/15')}>
            <Folder className="size-3.5 shrink-0" aria-hidden="true" />
            <span className="min-w-0 flex-1 truncate text-left">{selectedLabel}</span>
            <ChevronDown className="size-3.5 shrink-0 opacity-60" aria-hidden="true" />
          </button>
        </PopoverTrigger>
      </OverflowTooltip>
      <PopoverContent align="end" className="flex max-h-[min(24rem,var(--radix-popover-content-available-height))] w-max min-w-[min(16rem,calc(100vw-2rem))] max-w-[min(20rem,calc(100vw-2rem))] flex-col overflow-hidden p-0"
        onOpenAutoFocus={event => { event.preventDefault(); searchRef.current?.focus(); }}
        onEscapeKeyDown={event => event.stopPropagation()}>
        {/* 搜索框跟随选项确定的宽度，不参与弹层的固有宽度计算。 */}
        <div className="relative w-0 min-w-full shrink-0 border-b p-2">
          <Search className="pointer-events-none absolute left-4 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" aria-hidden="true" />
          <Input ref={searchRef} role="combobox" type="search" autoComplete="off" aria-label={t('skills.filter.searchSources')}
            aria-autocomplete="list" aria-expanded={open} aria-controls={listId} aria-activedescendant={`${listId}-${active}`}
            placeholder={t('skills.filter.searchSources')} value={query} className="h-8 pl-8 text-sm"
            onChange={event => { setQuery(event.target.value); setActiveIndex(0); }}
            onKeyDown={event => {
              if (event.nativeEvent.isComposing || event.keyCode === 229) return;
              if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
                event.preventDefault(); setActiveIndex((active + (event.key === 'ArrowDown' ? 1 : choices.length - 1)) % choices.length);
              } else if (event.key === 'Home' || event.key === 'End') {
                event.preventDefault(); setActiveIndex(event.key === 'Home' ? 0 : choices.length - 1);
              } else if (event.key === 'Enter') {
                event.preventDefault(); select(choices[active]);
              } else if (event.key === 'Escape') {
                event.preventDefault(); event.stopPropagation(); setOpen(false);
              }
            }} />
        </div>
        <div ref={listRef} id={listId} role="listbox" aria-label={t('skills.filter.sourceLabel')} className="min-h-0 overflow-y-auto p-1">
          {choices.map((option, index) => {
            const duplicates = option ? groups.get(`${option.kind}\0${option.label}`) ?? [] : [];
            const sameDetail = option ? duplicates.filter(other => other.detail === option.detail).sort((a, b) => a.key.localeCompare(b.key)) : [];
            const suffix = option && duplicates.length > 1
              ? sameDetail.length > 1 ? t('skills.filter.sourceNumber', { count: sameDetail.findIndex(other => other.key === option.key) + 1 }) : option.detail
              : '';
            const display = option ? `${sourceLabel(option, t)}${suffix ? ` · ${suffix}` : ''}` : t('skills.filter.allSources');
            const count = option?.count ?? totalCount;
            const Icon = option?.kind === 'wellKnown' ? Globe : option?.kind === 'dependency' ? Package : option?.kind === 'incomplete' ? CircleHelp : Folder;
            return <button key={option?.key ?? 'all'} id={`${listId}-${index}`} type="button" role="option" tabIndex={-1}
              aria-selected={(selected?.key ?? null) === (option?.key ?? null)} aria-label={`${display} (${count})`}
              title={option ? `${display} ${option.detail}` : display}
              onPointerMove={() => setActiveIndex(index)} onClick={() => select(option)}
              className={cn('flex w-full min-w-0 items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm', active === index && 'bg-accent text-accent-foreground')}>
              <Icon className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
              <span className="min-w-0 flex-1 truncate">{display}</span>
              <span className="shrink-0 text-xs tabular-nums text-muted-foreground">{count}</span>
              <Check className={cn('size-3.5 shrink-0', (selected?.key ?? null) !== (option?.key ?? null) && 'invisible')} aria-hidden="true" />
            </button>;
          })}
          {filtered.length === 0 && query.trim() ? <p className="px-3 py-4 text-center text-xs text-muted-foreground">{t('skills.filter.noMatchingSources')}</p> : null}
        </div>
      </PopoverContent>
    </Popover>
  );
}
