/* @vitest-environment jsdom */

import '@/test-utils';
import { act, render, screen, waitFor } from '@testing-library/react';
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

  it('renders the hot metric delta separately so it can use a distinct accent color', async () => {
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
    expect(deltaMetric.className).toContain('text-emerald-600');
  });
});