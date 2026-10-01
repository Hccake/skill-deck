/* @vitest-environment jsdom */

import '@/test-utils';
import { TooltipProvider } from '@/components/ui/tooltip';

const render = (ui: React.ReactNode) => renderComponent(ui, { wrapper: TooltipProvider });
import { act, fireEvent, render as renderComponent, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DiscoverListPanel } from '../DiscoverListPanel';
import type { DiscoverSkillSummary } from '@/lib/discover/types';

const mocks = vi.hoisted(() => ({
  getDiscoverLeaderboard: vi.fn(),
  searchDiscoverSkills: vi.fn(),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock('@/lib/discover/api', () => ({
  getDiscoverLeaderboard: (...args: unknown[]) => mocks.getDiscoverLeaderboard(...args),
  searchDiscoverSkills: (...args: unknown[]) => mocks.searchDiscoverSkills(...args),
}));

function makeSkill(overrides: Partial<DiscoverSkillSummary>): DiscoverSkillSummary {
  return {
    slug: 'same-name',
    name: 'find-skills',
    source: 'owner/skills',
    displayMetric: {
      kind: 'installs',
      rawText: '10.0K',
      sortValue: 10000,
    },
    isOfficial: false,
    detailUrl: 'https://skills.sh/owner/skills/find-skills',
    ...overrides,
  };
}

describe('DiscoverListPanel', () => {
  it('browses by keyboard without selecting until confirmed and keeps a single tab stop', async () => {
    const user = userEvent.setup();
    const skills = [makeSkill({ name: 'alpha' }), makeSkill({ name: 'beta', detailUrl: 'https://skills.sh/owner/skills/beta' })];
    mocks.getDiscoverLeaderboard.mockResolvedValue(skills);
    const onSelect = vi.fn();
    render(<DiscoverListPanel installedSkillLocations={new Map()} onSelect={onSelect}
      selectedDetailUrl={skills[0].detailUrl} activeTab="popular" onTabChange={vi.fn()} />);
    const alpha = await screen.findByRole('button', { name: /alpha/ });
    const beta = screen.getByRole('button', { name: /beta/ });
    expect([alpha.tabIndex, beta.tabIndex]).toEqual([0, -1]);
    act(() => alpha.focus());
    await user.keyboard('{ArrowDown}');
    expect(document.activeElement).toBe(beta);
    expect(alpha.getAttribute('aria-current')).toBe('true');
    expect(onSelect).not.toHaveBeenCalled();
    await user.keyboard('{Enter}');
    expect(onSelect).toHaveBeenLastCalledWith(skills[1]);
    expect(document.activeElement).toBe(beta);
    await user.keyboard('{Home}{ArrowUp}');
    expect(document.activeElement).toBe(alpha);
    await user.keyboard('{End}{ArrowDown} ');
    expect(onSelect).toHaveBeenCalledTimes(2);
    expect(document.activeElement).toBe(beta);
    fireEvent.keyDown(beta, { key: 'ArrowUp', isComposing: true });
    expect(document.activeElement).toBe(beta);
    await user.keyboard('{Control>}f{/Control}');
    expect(document.activeElement).toBe(screen.getByRole('searchbox'));
  });

  it('removes browse tabs during search, ignores stale results, and clears with focus retained', async () => {
    vi.useFakeTimers();
    let finishOldSearch!: (skills: ReturnType<typeof makeSkill>[]) => void;
    mocks.getDiscoverLeaderboard.mockResolvedValue([makeSkill({ name: 'popular' })]);
    mocks.searchDiscoverSkills.mockImplementationOnce(() => new Promise(resolve => { finishOldSearch = resolve; }))
      .mockResolvedValueOnce([makeSkill({ name: 'latest' })]);
    render(<DiscoverListPanel installedSkillLocations={new Map()} onSelect={vi.fn()}
      activeTab="popular" onTabChange={vi.fn()} />);
    await act(() => vi.advanceTimersByTimeAsync(250));
    const search = screen.getByRole('searchbox', { name: 'skills.discover.searchLabel' });
    search.focus();
    fireEvent.change(search, { target: { value: 'old' } });
    await act(() => vi.advanceTimersByTimeAsync(300));
    expect(screen.queryByRole('tab')).toBeNull();
    fireEvent.change(search, { target: { value: 'new' } });
    await act(() => vi.advanceTimersByTimeAsync(500));
    await act(async () => { finishOldSearch([makeSkill({ name: 'stale' })]); });
    expect(screen.getByRole('button', { name: /latest/ })).toBeTruthy();
    expect(screen.queryByText('stale')).toBeNull();
    expect(document.activeElement).toBe(search);
    expect(screen.getByRole('status').textContent).toContain('skills.discover.searchResults');
    fireEvent.keyDown(search, { key: 'Escape', isComposing: true });
    expect((search as HTMLInputElement).value).toBe('new');
    fireEvent.keyDown(search, { key: 'Escape' });
    expect((search as HTMLInputElement).value).toBe('');
    expect(screen.getAllByRole('tab')).toHaveLength(3);
    fireEvent.change(search, { target: { value: 'clear' } });
    const clear = screen.getByRole('button', { name: 'skills.discover.clearSearch' });
    clear.focus();
    fireEvent.click(clear);
    expect(document.activeElement).toBe(search);
    expect((search as HTMLInputElement).value).toBe('');
    await act(() => vi.advanceTimersByTimeAsync(500));
  });

  it('retries the current search and gives a removed retry button a stable focus destination', async () => {
    vi.useFakeTimers();
    mocks.getDiscoverLeaderboard.mockResolvedValue([]);
    mocks.searchDiscoverSkills.mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValueOnce([makeSkill({ name: 'recovered' })]);
    render(<DiscoverListPanel installedSkillLocations={new Map()} onSelect={vi.fn()}
      activeTab="popular" onTabChange={vi.fn()} />);
    const search = screen.getByRole('searchbox');
    fireEvent.change(search, { target: { value: 'keep this query' } });
    await act(() => vi.advanceTimersByTimeAsync(500));
    expect(screen.getByRole('alert').textContent).toContain('skills.discover.error');
    const retry = screen.getByRole('button', { name: 'skills.discover.retry' });
    retry.focus();
    fireEvent.click(retry);
    expect(document.activeElement).toBe(search);
    await act(() => vi.advanceTimersByTimeAsync(500));
    expect(screen.getByRole('button', { name: /recovered/ })).toBeTruthy();
    expect(mocks.searchDiscoverSkills).toHaveBeenLastCalledWith('keep this query');
    expect((search as HTMLInputElement).value).toBe('keep this query');
    expect(document.activeElement).toBe(search);
  });

  beforeEach(() => {
    mocks.getDiscoverLeaderboard.mockReset();
    mocks.searchDiscoverSkills.mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('matches selected item by detail url instead of slug alone', async () => {
    mocks.getDiscoverLeaderboard.mockResolvedValue([
      makeSkill({ source: 'first/skills', detailUrl: 'https://skills.sh/first/skills/find-skills' }),
      makeSkill({ source: 'second/skills', detailUrl: 'https://skills.sh/second/skills/find-skills' }),
    ]);

    render(
      <DiscoverListPanel
        installedSkillLocations={new Map()}
        onSelect={() => undefined}
        selectedDetailUrl="https://skills.sh/second/skills/find-skills"
        activeTab="popular"
        onTabChange={() => undefined}
      />
    );

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 80));
    });

    await waitFor(() => {
      expect(screen.getAllByText('find-skills')).toHaveLength(2);
    });

    expect(screen.queryAllByTestId('discover-skill-item').map((element) => element.getAttribute('data-selected'))).toEqual([
      'false',
      'true',
    ]);
  });

  it('shows skeleton again when switching tabs after cached data is available', async () => {
    vi.useFakeTimers();

    mocks.getDiscoverLeaderboard
      .mockResolvedValueOnce([makeSkill({ detailUrl: 'https://skills.sh/owner/skills/alpha' })])
      .mockResolvedValueOnce([makeSkill({ detailUrl: 'https://skills.sh/owner/skills/beta' })]);

    const { rerender } = render(
      <DiscoverListPanel
        installedSkillLocations={new Map()}
        onSelect={() => undefined}
        activeTab="popular"
        onTabChange={() => undefined}
      />
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(260);
    });

    rerender(
      <DiscoverListPanel
        installedSkillLocations={new Map()}
        onSelect={() => undefined}
        activeTab="trending"
        onTabChange={() => undefined}
      />
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(50);
    });

    expect(screen.getByTestId('discover-list-skeleton')).toBeTruthy();
  });

  it('renders the hot metric value and delta separately', async () => {
    mocks.getDiscoverLeaderboard.mockResolvedValue([
      makeSkill({
        detailUrl: 'https://skills.sh/docs/stripe-best-practices',
        displayMetric: {
          kind: 'hot',
          rawText: '22 +10',
          sortValue: 22,
        },
      }),
    ]);

    render(
      <DiscoverListPanel
        installedSkillLocations={new Map()}
        onSelect={() => undefined}
        activeTab="hot"
        onTabChange={() => undefined}
      />
    );

    await waitFor(() => {
      expect(screen.getByTestId('discover-hot-metric-value')).toBeTruthy();
    });

    const primaryMetric = screen.getByTestId('discover-hot-metric-value');
    const deltaMetric = screen.getByTestId('discover-hot-metric-delta');

    expect(primaryMetric.textContent).toBe('22');
    expect(deltaMetric.textContent).toBe('+10');
  });
});
