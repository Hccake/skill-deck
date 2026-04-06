/* @vitest-environment jsdom */

import '@/test-utils';
import '@/index.css';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DiscoverDetailPanel } from '../DiscoverDetailPanel';
import type { DiscoverSkillSummary } from '@/lib/discover/types';

const mocks = vi.hoisted(() => ({
  getDiscoverSkillDetail: vi.fn(),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock('@/lib/discover/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/discover/api')>('@/lib/discover/api');
  return {
    ...actual,
    getDiscoverSkillDetail: (...args: unknown[]) => mocks.getDiscoverSkillDetail(...args),
  };
});

function makeSkill(overrides: Partial<DiscoverSkillSummary>): DiscoverSkillSummary {
  return {
    slug: 'find-skills',
    name: 'find-skills',
    source: 'vercel-labs/skills',
    displayMetric: {
      kind: 'installs',
      rawText: '787.5K',
      sortValue: 787500,
    },
    installs: 787500,
    isOfficial: true,
    detailUrl: 'https://skills.sh/vercel-labs/skills/find-skills',
    ...overrides,
  };
}

describe('DiscoverDetailPanel', () => {
  beforeEach(() => {
    mocks.getDiscoverSkillDetail.mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders skill content and hides unknown risk badge', async () => {
    mocks.getDiscoverSkillDetail.mockResolvedValue({
      ...makeSkill({}),
      description: 'Discover and install specialized agent skills from the open ecosystem.',
      summaryHtml: '<p><strong>Discover and install specialized agent skills from the open ecosystem.</strong></p><ul><li>Helps identify relevant skills by domain and task</li></ul>',
      highlights: ['Helps identify relevant skills by domain and task'],
      installCommand: 'npx skills add https://github.com/vercel-labs/skills --skill find-skills',
      repoUrl: 'https://github.com/vercel-labs/skills',
      weeklyInstalls: 847500,
      stars: 12800,
      firstSeen: 'Jan 26, 2026',
      securityAudits: [
        { name: 'Socket Pass', status: 'pass', url: 'https://skills.sh/socket' },
        { name: 'Snyk Warn', status: 'warn', url: 'https://skills.sh/snyk' },
      ],
      installedOn: [
        { agent: 'codex', installsText: '787.5K', installs: 787500 },
      ],
      auditRisk: 'unknown',
      contentHtml: '<h2>Usage</h2><p>Rendered article content</p>',
    });

    render(
      <DiscoverDetailPanel
        skill={makeSkill({})}
        installLocations={[]}
        onClose={() => undefined}
        onInstall={() => undefined}
      />
    );

    await waitFor(() => {
      expect(screen.getByText('skills.discover.installViaCli')).toBeTruthy();
    });

    expect(screen.queryByText('skills.discover.riskBadge.unknown')).toBeNull();
    expect(screen.getByText('skills.discover.weeklyInstalls')).toBeTruthy();
    expect(screen.getByText('skills.discover.starsLabel')).toBeTruthy();
    expect(screen.getByText('Helps identify relevant skills by domain and task')).toBeTruthy();
    expect(screen.queryByText('npx skills add https://github.com/vercel-labs/skills --skill find-skills')).toBeNull();

    fireEvent.click(screen.getByText('skills.discover.installViaCli'));

    const overviewList = screen.getByText('Helps identify relevant skills by domain and task').closest('li')?.parentElement;
  const overviewProse = screen.getByText('Helps identify relevant skills by domain and task').closest('.skill-prose');

    expect(overviewList?.tagName).toBe('UL');
    expect(overviewList ? getComputedStyle(overviewList).listStyleType : '').toBe('disc');
  expect(overviewProse?.className).toContain('skill-prose-with-lists');
    expect(screen.getByText('npx skills add https://github.com/vercel-labs/skills --skill find-skills')).toBeTruthy();
    expect(screen.getByText('Rendered article content')).toBeTruthy();
    expect(screen.getByText('Usage')).toBeTruthy();
  });

  it('shows skeleton again when switching skills after a detail was loaded', async () => {
    vi.useFakeTimers();

    mocks.getDiscoverSkillDetail
      .mockResolvedValueOnce({
        ...makeSkill({}),
        highlights: [],
        securityAudits: [],
        installedOn: [],
        contentHtml: '<p>Alpha</p>',
      })
      .mockResolvedValueOnce({
        ...makeSkill({
          detailUrl: 'https://skills.sh/vercel-labs/skills/other-skill',
          name: 'other-skill',
          slug: 'other-skill',
        }),
        highlights: [],
        securityAudits: [],
        installedOn: [],
        contentHtml: '<p>Beta</p>',
      });

    const { rerender } = render(
      <DiscoverDetailPanel
        skill={makeSkill({})}
        installLocations={[]}
        onClose={() => undefined}
        onInstall={() => undefined}
      />
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(220);
    });

    expect(screen.getByText('Alpha')).toBeTruthy();

    rerender(
      <DiscoverDetailPanel
        skill={makeSkill({
          detailUrl: 'https://skills.sh/vercel-labs/skills/other-skill',
          name: 'other-skill',
          slug: 'other-skill',
        })}
        installLocations={[]}
        onClose={() => undefined}
        onInstall={() => undefined}
      />
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    expect(screen.getByTestId('discover-detail-skeleton')).toBeTruthy();
  });
});