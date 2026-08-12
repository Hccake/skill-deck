/* @vitest-environment jsdom */

import '@/test-utils';
import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { AgentFilterCombobox } from '../AgentFilterCombobox';
import { makeResolvedAgent } from '@/test-utils';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => ({
      'skills.filter.allAgents': '全部 Agents',
      'skills.filter.agentLabel': '按 Agent 筛选 Skill',
      'skills.filter.searchAgents': '查找 Agent…',
    }[key] ?? key),
  }),
}));

describe('AgentFilterCombobox', () => {
  it('shows 全部 Agents before selection and only the Agent name after selection', async () => {
    const onChange = vi.fn();
    const agents = [
      makeResolvedAgent({ id: 'codex', displayName: 'Codex' }),
      makeResolvedAgent({ id: 'cursor', displayName: 'Cursor' }),
    ];

    const { rerender } = render(
      <AgentFilterCombobox
        agents={agents}
        selectedAgent={null}
        onChange={onChange}
        matchCounts={new Map([['codex', 3], ['cursor', 1]])}
        totalSkillCount={4}
      />,
    );

    const trigger = screen.getByRole('button', { name: '按 Agent 筛选 Skill' });
    expect(trigger.textContent).toContain('全部 Agents');
    fireEvent.click(trigger);

    const search = screen.getByRole('combobox', { name: '查找 Agent…' });
    expect(screen.getByRole('option', { name: 'Codex (3)' })).toBeDefined();
    expect(screen.getByRole('option', { name: 'Cursor (1)' })).toBeDefined();

    fireEvent.change(search, { target: { value: 'cur' } });
    expect(screen.queryByRole('option', { name: 'Codex (3)' })).toBeNull();
    expect(screen.getByRole('option', { name: 'Cursor (1)' })).toBeDefined();

    fireEvent.click(screen.getByRole('option', { name: 'Cursor (1)' }));
    expect(onChange).toHaveBeenCalledWith('cursor');

    rerender(
      <AgentFilterCombobox
        agents={agents}
        selectedAgent="cursor"
        onChange={onChange}
        matchCounts={new Map([['codex', 3], ['cursor', 1]])}
        totalSkillCount={4}
      />,
    );
    expect(screen.getByRole('button', { name: '按 Agent 筛选 Skill' }).textContent)
      .toContain('Cursor');
    expect(screen.queryByRole('button', { name: 'skills.filter.clearAgent' })).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: '按 Agent 筛选 Skill' }));
    fireEvent.click(screen.getByRole('option', { name: '全部 Agents (4)' }));
    expect(onChange).toHaveBeenLastCalledWith(null);

    await waitFor(() => {
      expect(screen.queryByRole('combobox')).toBeNull();
    });
  });

  it('does not reserve all as a sentinel value', () => {
    const onChange = vi.fn();
    const allAgent = makeResolvedAgent({ id: 'all', displayName: 'All Tools' });

    render(
      <AgentFilterCombobox
        agents={[allAgent]}
        selectedAgent={null}
        onChange={onChange}
        matchCounts={new Map([['all', 2]])}
        totalSkillCount={2}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: '按 Agent 筛选 Skill' }));
    fireEvent.click(screen.getByRole('option', { name: 'All Tools (2)' }));

    expect(onChange).toHaveBeenCalledWith('all');
  });

  it('keeps the selected Agent name while the next Context is loading', () => {
    const onChange = vi.fn();
    const codex = makeResolvedAgent({ id: 'codex', displayName: 'Codex' });
    const { rerender } = render(
      <AgentFilterCombobox
        agents={[codex]}
        selectedAgent={null}
        onChange={onChange}
        matchCounts={new Map([['codex', 1]])}
        totalSkillCount={1}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: '按 Agent 筛选 Skill' }));
    fireEvent.click(screen.getByRole('option', { name: 'Codex (1)' }));
    expect(onChange).toHaveBeenCalledWith('codex');

    rerender(
      <AgentFilterCombobox
        agents={[codex]}
        selectedAgent="codex"
        onChange={onChange}
        matchCounts={new Map([['codex', 1]])}
        totalSkillCount={1}
      />,
    );
    rerender(
      <AgentFilterCombobox
        agents={[]}
        selectedAgent="codex"
        onChange={onChange}
        matchCounts={new Map()}
        totalSkillCount={0}
      />,
    );

    const trigger = screen.getByRole('button', { name: '按 Agent 筛选 Skill' });
    expect(trigger.textContent).toContain('Codex');
    expect(trigger.textContent).not.toContain('codex');
  });

  it('supports keyboard selection from the search field', () => {
    const onChange = vi.fn();

    render(
      <AgentFilterCombobox
        agents={[makeResolvedAgent({ id: 'codex', displayName: 'Codex' })]}
        selectedAgent={null}
        onChange={onChange}
        matchCounts={new Map([['codex', 1]])}
        totalSkillCount={1}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: '按 Agent 筛选 Skill' }));
    const search = screen.getByRole('combobox', { name: '查找 Agent…' });
    fireEvent.keyDown(search, { key: 'ArrowDown' });
    fireEvent.keyDown(search, { key: 'Enter' });

    expect(onChange).toHaveBeenCalledWith('codex');
    expect(screen.queryByRole('listbox')).toBeNull();
  });

  it('handles Escape inside the popup without bubbling to the detail panel', () => {
    const onParentEscape = vi.fn();

    render(
      <div onKeyDown={(event) => {
        if (event.key === 'Escape') onParentEscape();
      }}>
        <AgentFilterCombobox
          agents={[makeResolvedAgent({ id: 'codex', displayName: 'Codex' })]}
          selectedAgent={null}
          onChange={vi.fn()}
          matchCounts={new Map([['codex', 1]])}
          totalSkillCount={1}
        />
      </div>,
    );

    fireEvent.click(screen.getByRole('button', { name: '按 Agent 筛选 Skill' }));
    fireEvent.keyDown(screen.getByRole('combobox', { name: '查找 Agent…' }), {
      key: 'Escape',
    });

    expect(onParentEscape).not.toHaveBeenCalled();
    expect(screen.queryByRole('listbox')).toBeNull();
  });
});
