import { TooltipProvider } from '@/components/ui/tooltip';
import type { ReactElement } from 'react';
/* @vitest-environment jsdom */

import '@/test-utils';
import { render as renderBase, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import {
  SourceSkillSelectionPanel,
  type SourceSkillCandidate,
} from '../SourceSkillSelectionPanel';

const render = (ui: ReactElement) => renderBase(<TooltipProvider>{ui}</TooltipProvider>);

const candidates: SourceSkillCandidate[] = [
  {
    candidateId: 'candidate-alpha',
    name: 'alpha',
    description: 'Frontend utility',
    groupName: 'frontend-pack',
    selectable: true,
  },
  {
    candidateId: 'candidate-beta',
    name: 'beta',
    description: 'Backend utility',
    selectable: true,
  },
  {
    candidateId: 'candidate-existing',
    name: 'existing',
    description: 'Frontend installed Skill',
    selectable: false,
    statusLabel: 'Already in Library',
  },
];

const copy = {
  title: 'Choose Skills',
  selected: (count: number, total: number) => `${count} of ${total}`,
  searchPlaceholder: 'Search Skills',
  selectAll: 'Select visible',
  clear: 'Clear',
  empty: 'No Skills',
  generalGroup: 'General',
};

function Harness({ onSelectionChange = () => undefined }: {
  onSelectionChange?: (candidateIds: string[]) => void;
}) {
  const [query, setQuery] = useState('');
  const [selected, setSelected] = useState(['candidate-beta']);

  return (
    <SourceSkillSelectionPanel
      candidates={candidates}
      selectedCandidateIds={selected}
      query={query}
      onQueryChange={setQuery}
      onSelectionChange={(next) => {
        setSelected(next);
        onSelectionChange(next);
      }}
      copy={copy}
    />
  );
}

describe('SourceSkillSelectionPanel', () => {
  it('keeps ambiguous choices unselected and shows paths within their rows', async () => {
    const changed = vi.fn();
    render(<SourceSkillSelectionPanel candidates={[
      { candidateId: 'one', name: 'Code Review', description: 'first', relativePath: 'skills/one/SKILL.md', selectable: true },
      { candidateId: 'two', name: 'code_review', description: 'second', relativePath: 'tools/two/SKILL.md', selectable: true },
    ]} selectedCandidateIds={[]} query="" onQueryChange={() => {}} onSelectionChange={changed} copy={copy}
      selectionResult={{ matchedNames: [], unmatchedInputs: ['missing'], ambiguousInputs: [{ input: 'code-review', candidateNames: ['Code Review', 'code_review'] }] }} />);
    expect(screen.getByRole('status')).toBeTruthy();
    expect(screen.getByText('skills/one')).toBeTruthy();
    expect(screen.getByText('tools/two')).toBeTruthy();
    expect(screen.getByRole('checkbox', { name: 'Code Review' }).getAttribute('aria-checked')).toBe('false');
    await userEvent.click(screen.getByRole('checkbox', { name: 'Code Review' }));
    expect(changed).toHaveBeenCalledWith(['one']);
  });

  it('uses its title as an accessible region name without repeating a visible heading', () => {
    render(<Harness />);

    expect(screen.getByRole('region', { name: 'Choose Skills' })).toBeTruthy();
    expect(screen.queryByRole('heading', { name: 'Choose Skills' })).toBeNull();
  });

  it('selects only visible selectable candidates while preserving hidden selections', async () => {
    const user = userEvent.setup();
    const onSelectionChange = vi.fn();
    render(<Harness onSelectionChange={onSelectionChange} />);

    await user.type(screen.getByRole('searchbox', { name: 'Search Skills' }), 'frontend');
    await user.click(screen.getByRole('button', { name: 'Select visible' }));

    expect(onSelectionChange).toHaveBeenLastCalledWith([
      'candidate-beta',
      'candidate-alpha',
    ]);
  });

  it('shows target status and does not select a disabled candidate', async () => {
    const user = userEvent.setup();
    const onSelectionChange = vi.fn();
    render(<Harness onSelectionChange={onSelectionChange} />);

    expect(screen.getByText('Already in Library')).toBeTruthy();
    const existing = screen.getByRole('checkbox', { name: /existing/i });
    expect(existing).toHaveProperty('disabled', true);

    await user.click(existing);
    expect(onSelectionChange).not.toHaveBeenCalled();
  });

  it('toggles a selectable candidate from the complete labelled row', async () => {
    const user = userEvent.setup();
    const onSelectionChange = vi.fn();
    render(<Harness onSelectionChange={onSelectionChange} />);

    await user.click(screen.getByText('alpha'));
    expect(onSelectionChange).toHaveBeenLastCalledWith(['candidate-beta', 'candidate-alpha']);
  });

  it('clears the complete selection and reports candidate counts', async () => {
    const user = userEvent.setup();
    const onSelectionChange = vi.fn();
    render(<Harness onSelectionChange={onSelectionChange} />);

    expect(screen.getByText('1 of 2')).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Clear' }));
    expect(onSelectionChange).toHaveBeenLastCalledWith([]);
  });
});
