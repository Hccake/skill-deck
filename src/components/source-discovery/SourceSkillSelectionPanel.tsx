import { memo, useCallback, useMemo } from 'react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import { toTitleCase } from '@/lib/utils';

export interface SourceSkillCandidate {
  candidateId: string;
  name: string;
  description: string;
  groupName?: string | null;
  selectable: boolean;
  statusLabel?: string;
}

export interface SourceSkillSelectionCopy {
  title: string;
  selected: (count: number, total: number) => string;
  searchPlaceholder: string;
  selectAll: string;
  clear: string;
  empty: string;
  generalGroup: string;
}

interface SourceSkillSelectionPanelProps {
  candidates: SourceSkillCandidate[];
  selectedCandidateIds: string[];
  query: string;
  onQueryChange: (query: string) => void;
  onSelectionChange: (candidateIds: string[]) => void;
  copy: SourceSkillSelectionCopy;
}

export function SourceSkillSelectionPanel({
  candidates,
  selectedCandidateIds,
  query,
  onQueryChange,
  onSelectionChange,
  copy,
}: SourceSkillSelectionPanelProps) {
  const filteredCandidates = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    if (!normalizedQuery) return candidates;
    return candidates.filter((candidate) => (
      candidate.name.toLocaleLowerCase().includes(normalizedQuery)
      || candidate.description.toLocaleLowerCase().includes(normalizedQuery)
    ));
  }, [candidates, query]);
  const selectedSet = useMemo(
    () => new Set(selectedCandidateIds),
    [selectedCandidateIds],
  );
  const selectedCount = useMemo(
    () => candidates.reduce(
      (count, candidate) => count + Number(
        candidate.selectable && selectedSet.has(candidate.candidateId),
      ),
      0,
    ),
    [candidates, selectedSet],
  );
  const selectableCount = useMemo(
    () => candidates.reduce(
      (count, candidate) => count + Number(candidate.selectable),
      0,
    ),
    [candidates],
  );
  const groupedCandidates = useMemo(() => {
    const groups = new Map<string, SourceSkillCandidate[]>();
    const ungrouped: SourceSkillCandidate[] = [];

    for (const candidate of filteredCandidates) {
      if (!candidate.groupName) {
        ungrouped.push(candidate);
        continue;
      }
      const group = groups.get(candidate.groupName) ?? [];
      group.push(candidate);
      groups.set(candidate.groupName, group);
    }

    return groups.size > 0 ? { groups, ungrouped } : null;
  }, [filteredCandidates]);

  const toggleCandidate = useCallback((candidateId: string) => {
    const candidate = candidates.find((item) => item.candidateId === candidateId);
    if (!candidate?.selectable) return;

    const next = new Set(selectedCandidateIds);
    if (next.has(candidateId)) next.delete(candidateId);
    else next.add(candidateId);
    onSelectionChange([...next]);
  }, [candidates, onSelectionChange, selectedCandidateIds]);

  const selectVisible = useCallback(() => {
    const next = new Set(selectedCandidateIds);
    for (const candidate of filteredCandidates) {
      if (candidate.selectable) next.add(candidate.candidateId);
    }
    onSelectionChange([...next]);
  }, [filteredCandidates, onSelectionChange, selectedCandidateIds]);

  const renderCandidate = (candidate: SourceSkillCandidate) => (
    <CandidateRow
      key={candidate.candidateId}
      candidate={candidate}
      selected={selectedSet.has(candidate.candidateId)}
      onToggle={toggleCandidate}
    />
  );

  return (
    <div
      role="region"
      aria-label={copy.title}
      className="flex h-full min-h-0 flex-col gap-3"
    >
      <div className="flex shrink-0 flex-wrap items-center gap-2">
        <Input
          type="search"
          name="source-skill-search"
          autoComplete="off"
          spellCheck={false}
          value={query}
          onChange={(event) => onQueryChange(event.target.value)}
          aria-label={copy.searchPlaceholder}
          placeholder={copy.searchPlaceholder}
          className="h-9 min-w-48 flex-1"
        />
        <span className="shrink-0 text-sm tabular-nums text-muted-foreground">
          {copy.selected(selectedCount, selectableCount)}
        </span>
        <div className="flex shrink-0 items-center gap-1">
          <Button type="button" variant="ghost" size="sm" onClick={selectVisible}>
            {copy.selectAll}
          </Button>
          <Button type="button" variant="ghost" size="sm" onClick={() => onSelectionChange([])}>
            {copy.clear}
          </Button>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain rounded-md border bg-background">
        {filteredCandidates.length === 0 ? (
          <div className="p-8 text-center text-sm text-muted-foreground">{copy.empty}</div>
        ) : groupedCandidates ? (
          <>
            {[...groupedCandidates.groups.entries()]
              .sort(([left], [right]) => left.localeCompare(right))
              .map(([groupName, groupCandidates]) => (
                <section key={groupName} aria-label={toTitleCase(groupName)}>
                  <div className="border-b bg-muted/30 px-3 py-1.5 text-xs font-medium text-muted-foreground">
                    {toTitleCase(groupName)}
                  </div>
                  {groupCandidates.map(renderCandidate)}
                </section>
              ))}
            {groupedCandidates.ungrouped.length > 0 ? (
              <section aria-label={copy.generalGroup}>
                <div className="border-b bg-muted/30 px-3 py-1.5 text-xs font-medium text-muted-foreground">
                  {copy.generalGroup}
                </div>
                {groupedCandidates.ungrouped.map(renderCandidate)}
              </section>
            ) : null}
          </>
        ) : filteredCandidates.map(renderCandidate)}
      </div>
    </div>
  );
}

const CandidateRow = memo(function CandidateRow({
  candidate,
  selected,
  onToggle,
}: {
  candidate: SourceSkillCandidate;
  selected: boolean;
  onToggle: (candidateId: string) => void;
}) {
  return (
    <label
      className={`flex min-w-0 items-start gap-3 border-b px-3 py-2.5 last:border-b-0 ${
        candidate.selectable ? 'cursor-pointer' : 'cursor-not-allowed bg-muted/15'
      }`}
    >
      <Checkbox
        checked={selected}
        disabled={!candidate.selectable}
        onCheckedChange={() => onToggle(candidate.candidateId)}
        aria-label={candidate.name}
        className="mt-0.5 shrink-0"
      />
      <span className="min-w-0 flex-1">
        <span className="block break-words text-sm font-medium [overflow-wrap:anywhere]" translate="no">
          {candidate.name}
        </span>
        {candidate.description ? (
          <span className="mt-0.5 block break-words text-xs leading-5 text-muted-foreground [overflow-wrap:anywhere]">
            {candidate.description}
          </span>
        ) : null}
      </span>
      {candidate.statusLabel ? (
        <Badge variant="outline" className="max-w-40 shrink-0 whitespace-normal text-right">
          {candidate.statusLabel}
        </Badge>
      ) : null}
    </label>
  );
});
