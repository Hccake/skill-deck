/* @vitest-environment jsdom */
import '@/test-utils';
import { useState } from 'react';
import { createPortal } from 'react-dom';
import { act, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { useSkillListKeyboard } from '../useSkillListKeyboard';

const items = ['project:one:same', 'global:same', 'global:writing'];

function List({ keys = items, compact = true, selected = items[0], context = 'native:one', searchVisible = true, onActivate = () => {} }: {
  keys?: string[]; compact?: boolean; selected?: string; context?: string; searchVisible?: boolean; onActivate?: () => void;
}) {
  const keyboard = useSkillListKeyboard({ compact, selectedKey: selected, contextKey: context });
  const [portal, setPortal] = useState(false);
  return <>
    <div {...keyboard}>
      {searchVisible && <input type="search" aria-label="Search" data-skill-search />}
      <button onClick={() => setPortal(true)}>Dialog</button>
      {keys.map((key) => <div key={`${compact}:${key}`} data-skill-item={key}>
        <button data-skill-open onClick={onActivate}>{key}</button>
        {!compact && <button>Remove {key}</button>}
      </div>)}
      {portal && createPortal(<div role="dialog"><input aria-label="Dialog input" /></div>, document.body)}
    </div>
    <button>Detail command</button>
  </>;
}

describe('useSkillListKeyboard', () => {
  it('uses one tab stop, moves focus in visible order without activating, and retains native activation', async () => {
    const user = userEvent.setup();
    const activate = vi.fn();
    render(<List onActivate={activate} />);
    const rows = items.map((name) => screen.getByRole('button', { name }));
    expect(rows.map((row) => row.tabIndex)).toEqual([0, -1, -1]);
    rows[0].focus();
    await user.keyboard('{ArrowDown}{End}');
    expect(document.activeElement).toBe(rows[2]);
    expect(activate).not.toHaveBeenCalled();
    await user.keyboard('{Home}{ArrowUp}');
    expect(document.activeElement).toBe(rows[0]);
    await user.keyboard('{ArrowDown}{Enter}');
    expect(activate).toHaveBeenCalledTimes(1);
    await user.tab();
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Detail command' }));
    expect(rows.map((row) => row.tabIndex)).toEqual([0, -1, -1]);
  });

  it('hands focus between the same scoped card and row and restores the last selection on close', () => {
    const view = render(<List compact={false} selected="" />);
    screen.getByRole('button', { name: items[1] }).focus();
    view.rerender(<List selected={items[1]} />);
    expect(document.activeElement).toBe(screen.getByRole('button', { name: items[1] }));
    screen.getByRole('button', { name: 'Detail command' }).focus();
    view.rerender(<List compact={false} selected="" />);
    expect(document.activeElement).toBe(screen.getByRole('button', { name: items[1] }));
  });

  it('restores a removed focused item to its next neighbour, then the previous one, then search', () => {
    const view = render(<List />);
    screen.getByRole('button', { name: items[1] }).focus();
    view.rerender(<List keys={[items[0], items[2]]} />);
    expect(document.activeElement).toBe(screen.getByRole('button', { name: items[2] }));
    view.rerender(<List keys={[items[0]]} />);
    expect(document.activeElement).toBe(screen.getByRole('button', { name: items[0] }));
    view.rerender(<List keys={[]} />);
    expect(document.activeElement).toBe(screen.getByRole('searchbox'));
  });

  it('keeps editing focus through filtering and does not restore a previous context identity', () => {
    const view = render(<List />);
    screen.getByRole('button', { name: items[1] }).focus();
    const search = screen.getByRole('searchbox');
    search.focus();
    view.rerender(<List keys={[items[2]]} />);
    expect(document.activeElement).toBe(search);
    view.rerender(<List keys={items} selected="" context="wsl:two" />);
    expect(document.activeElement).toBe(search);
    expect(items.map((name) => screen.getByRole('button', { name }).tabIndex)).toEqual([0, -1, -1]);
  });

  it('returns to an available command when the empty list no longer has a search field', () => {
    const view = render(<List keys={[items[0]]} />);
    screen.getByRole('button', { name: items[0] }).focus();
    view.rerender(<List keys={[]} searchVisible={false} />);
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Dialog' }));
  });

  it('limits find to its list and ignores modified navigation, editing, IME and portal events', async () => {
    const user = userEvent.setup();
    render(<List />);
    const row = screen.getByRole('button', { name: items[0] });
    row.focus();
    fireEvent.keyDown(row, { key: 'ArrowDown', ctrlKey: true });
    fireEvent.keyDown(row, { key: 'ArrowDown', isComposing: true });
    expect(document.activeElement).toBe(row);
    await user.keyboard('{Meta>}f{/Meta}');
    const search = screen.getByRole('searchbox');
    expect(document.activeElement).toBe(search);
    await user.keyboard('{ArrowDown}{Home}');
    expect(document.activeElement).toBe(search);
    await user.click(screen.getByRole('button', { name: 'Dialog' }));
    const input = screen.getByRole('textbox', { name: 'Dialog input' });
    input.focus();
    await user.keyboard('{Control>}f{/Control}');
    expect(document.activeElement).toBe(input);
  });

  it('preserves dialog focus when a list update removes the old trigger', async () => {
    const view = render(<List compact={false} />);
    screen.getByRole('button', { name: `Remove ${items[0]}` }).focus();
    fireEvent.click(screen.getByRole('button', { name: 'Dialog' }));
    const input = screen.getByRole('textbox', { name: 'Dialog input' });
    act(() => input.focus());
    view.rerender(<List compact={false} keys={[items[1]]} />);
    expect(document.activeElement).toBe(input);
  });
});
