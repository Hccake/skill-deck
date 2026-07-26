/* @vitest-environment jsdom */

import '@/test-utils';
import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { SkillsToolbar } from '../SkillsToolbar';
import { makeResolvedAgent } from '@/test-utils';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

function renderToolbar(compact = false) {
  return render(
    <SkillsToolbar
      compact={compact}
      searchQuery=""
      onSearchChange={vi.fn()}
      selectedAgent={null}
      onAgentChange={vi.fn()}
      filterableAgents={[makeResolvedAgent({ id: 'codex', displayName: 'Codex' })]}
      agentMatchCounts={new Map([['codex', 2]])}
      totalSkillCount={3}
      hasActiveFilters={false}
      onClearFilters={vi.fn()}
      onSync={vi.fn()}
    />,
  );
}

describe('SkillsToolbar', () => {
  it('keeps the Agent filter available in compact mode with accessible names', () => {
    renderToolbar(true);

    expect(screen.getByRole('searchbox', { name: 'skills.search' })).toBeDefined();
    expect(screen.getByRole('button', { name: 'skills.filter.agentLabel' })).toBeDefined();
    expect(screen.queryByRole('button', { name: 'skills.sync' })).toBeNull();
  });

  it('keeps one clear-all action without reserving space for a result counter', () => {
    const onClearFilters = vi.fn();
    render(
      <SkillsToolbar
        searchQuery="writer"
        onSearchChange={vi.fn()}
        selectedAgent="codex"
        onAgentChange={vi.fn()}
        filterableAgents={[makeResolvedAgent({ id: 'codex', displayName: 'Codex' })]}
        agentMatchCounts={new Map([['codex', 1]])}
        totalSkillCount={4}
        hasActiveFilters
        onClearFilters={onClearFilters}
        onSync={vi.fn()}
      />,
    );

    expect(screen.queryByRole('status')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'skills.filter.clear' }));
    expect(onClearFilters).toHaveBeenCalledTimes(1);
  });

  it('clears search on Escape without closing the parent surface', () => {
    const onSearchChange = vi.fn();
    const onParentKeyDown = vi.fn();
    render(
      <div onKeyDown={onParentKeyDown}>
        <SkillsToolbar
          searchQuery="writer"
          onSearchChange={onSearchChange}
          selectedAgent={null}
          onAgentChange={vi.fn()}
          filterableAgents={[]}
          agentMatchCounts={new Map()}
          totalSkillCount={4}
          hasActiveFilters
          onClearFilters={vi.fn()}
          onSync={vi.fn()}
        />
      </div>,
    );

    fireEvent.keyDown(screen.getByRole('searchbox', { name: 'skills.search' }), {
      key: 'Escape',
    });

    expect(onSearchChange).toHaveBeenCalledWith('');
    expect(onParentKeyDown).not.toHaveBeenCalled();
  });
});
