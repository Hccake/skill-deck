import { beforeEach, describe, expect, expectTypeOf, it, vi } from 'vitest';
import searchReactFixture from './fixtures/search-react.json';
import detailFindSkillsHtml from './fixtures/detail-find-skills.html?raw';
import leaderboardHotHtml from './fixtures/leaderboard-hot.html?raw';
import leaderboardPopularHtml from './fixtures/leaderboard-popular.html?raw';
import leaderboardTrendingHtml from './fixtures/leaderboard-trending.html?raw';
import officialCreatorsHtml from './fixtures/official-creators.html?raw';
import {
  __resetDiscoverApiState,
  getDiscoverLeaderboard,
  getDiscoverSkillDetail,
  parseOfficialOwners,
  searchDiscoverSkills,
} from '../api';

const fetchMock = vi.fn();

vi.mock('@tauri-apps/plugin-http', () => ({
  fetch: (...args: unknown[]) => fetchMock(...args),
}));

function makeResponse(body: string, ok = true, status = 200) {
  return {
    ok,
    status,
    text: async () => body,
    json: async () => JSON.parse(body),
  };
}

function mockDiscoverFetch(overrides: Partial<Record<string, string>> = {}): void {
  const fixtures: Record<string, string> = {
    '/official': officialCreatorsHtml,
    '/': leaderboardPopularHtml,
    '/trending': leaderboardTrendingHtml,
    '/hot': leaderboardHotHtml,
    '/detail': detailFindSkillsHtml,
    ...overrides,
  };

  fetchMock.mockImplementation(async (url: string | URL) => {
    const href = String(url);

    if (href.includes('/api/search')) {
      throw new Error('Unexpected search request');
    }

    if (href.endsWith('/official')) return makeResponse(fixtures['/official']);
    if (href.endsWith('/trending')) return makeResponse(fixtures['/trending']);
    if (href.endsWith('/hot')) return makeResponse(fixtures['/hot']);
    if (href.endsWith('/vercel-labs/skills/find-skills')) return makeResponse(fixtures['/detail']);
    if (href === 'https://skills.sh') return makeResponse(fixtures['/']);

    throw new Error(`Unexpected URL: ${href}`);
  });
}

describe('discover api adapter', () => {
  beforeEach(() => {
    fetchMock.mockReset();
    __resetDiscoverApiState();
  });

  it('preserves search api order and derives official metadata from creator slugs', async () => {
    fetchMock.mockImplementation(async (url: string | URL) => {
      const href = String(url);
      if (href.includes('/api/search')) {
        return makeResponse(JSON.stringify({
          ...searchReactFixture,
          skills: [searchReactFixture.skills[1], searchReactFixture.skills[0]],
        }));
      }

      if (href.endsWith('/official')) {
        return makeResponse(officialCreatorsHtml);
      }

      throw new Error(`Unexpected URL: ${href}`);
    });

    const result = await searchDiscoverSkills('react');

    expect(result.map((skill) => skill.slug)).toEqual([
      'react:components',
      'vercel-react-best-practices',
    ]);
    expect(result[0].source).toBe('google-labs-code/stitch-skills');
    expect(result[0].displayMetric).toEqual({
      kind: 'installs',
      rawText: '26.1K',
      sortValue: 26055,
    });
    expect(result[0].isOfficial).toBe(false);
    expect(result[1].source).toBe('vercel-labs/agent-skills');
    expect(result[1].displayMetric.rawText).toBe('263.7K');
    expect(result[1].isOfficial).toBe(true);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('requests search results with a limit of 100', async () => {
    fetchMock.mockImplementation(async (url: string | URL) => {
      const href = String(url);
      if (href.includes('/api/search')) {
        return makeResponse(JSON.stringify(searchReactFixture));
      }

      if (href.endsWith('/official')) {
        return makeResponse(officialCreatorsHtml);
      }

      throw new Error(`Unexpected URL: ${href}`);
    });

    await searchDiscoverSkills('react');

    expect(String(fetchMock.mock.calls[0]?.[0])).toContain('limit=100');
  });

  it('extracts official creator slugs from single-segment links', () => {
    expect(Array.from(parseOfficialOwners(officialCreatorsHtml))).toEqual([
      'anthropics',
      'microsoft',
      'vercel-labs',
    ]);
  });

  it.each([
    {
      tab: 'popular' as const,
      fixture: 'leaderboard-popular.html',
      metricKind: 'installs' as const,
      metricText: '787.5K',
      metricValue: 787500,
      installs: 787500,
      official: true,
    },
    {
      tab: 'trending' as const,
      fixture: 'leaderboard-trending.html',
      metricKind: 'trending-24h' as const,
      metricText: '13.9K',
      metricValue: 13900,
      installs: undefined,
      official: true,
    },
    {
      tab: 'hot' as const,
      fixture: 'leaderboard-hot.html',
      metricKind: 'hot' as const,
      metricText: '248 +248',
      metricValue: 248,
      installs: undefined,
      official: false,
      expectedLength: 3,
    },
  ])('parses $tab leaderboard rows without fabricating summaries', async ({ tab, fixture, metricKind, metricText, metricValue, installs, official, expectedLength = 2 }) => {
    mockDiscoverFetch({
      '/': fixture === 'leaderboard-popular.html' ? leaderboardPopularHtml : leaderboardPopularHtml,
      '/trending': fixture === 'leaderboard-trending.html' ? leaderboardTrendingHtml : leaderboardTrendingHtml,
      '/hot': fixture === 'leaderboard-hot.html' ? leaderboardHotHtml : leaderboardHotHtml,
    });

    const result = await getDiscoverLeaderboard(tab);

    expect(result).toHaveLength(expectedLength);
    expect(result[0].slug).toBe(tab === 'hot' ? 'readme-i18n' : 'find-skills');
    expect(result[0].source).toBe(tab === 'hot' ? 'xixu-me/skills' : 'vercel-labs/skills');
    expect(result[0].summary).toBeUndefined();
    expect(result[0].displayMetric).toEqual({
      kind: metricKind,
      rawText: metricText,
      sortValue: metricValue,
    });
    expect(result[0].installs).toBe(installs);
    expect(result[0].isOfficial).toBe(official);
  });

  it('parses site-based hot leaderboard entries without folding name and source into metric text', async () => {
    mockDiscoverFetch({
      '/hot': leaderboardHotHtml,
    });

    const result = await getDiscoverLeaderboard('hot');
    const stripeSkill = result.find((skill) => skill.slug === 'stripe-best-practices');

    expect(stripeSkill).toMatchObject({
      source: 'docs.stripe.com',
      displayMetric: {
        kind: 'hot',
        rawText: '22 +10',
        sortValue: 22,
      },
      detailUrl: 'https://skills.sh/site/docs.stripe.com/stripe-best-practices',
    });
  });

  it('caches leaderboard responses with official creator metadata', async () => {
    mockDiscoverFetch();

    const first = await getDiscoverLeaderboard('popular');
    const second = await getDiscoverLeaderboard('popular');

    expect(second).toEqual(first);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('only exposes public leaderboard tabs in its signature', () => {
    type LeaderboardTab = Parameters<typeof getDiscoverLeaderboard>[0];
    expectTypeOf<LeaderboardTab>().toEqualTypeOf<'popular' | 'trending' | 'hot'>();
  });

  it('parses detail fields, K/M metrics, and scoped highlights only', async () => {
    mockDiscoverFetch();

    const detail = await getDiscoverSkillDetail('/vercel-labs/skills/find-skills');

    expect(detail.summary).toBe('Discover and install specialized agent skills from the open ecosystem.');
    expect(detail.description).toBe('Discover and install specialized agent skills from the open ecosystem.');
    expect(detail.summaryHtml).toContain('<ul>');
    expect(detail.summaryHtml).toContain('Use the results to compare candidate skills before installation.');
    expect(detail.installCommand).toBe('npx skills add https://github.com/vercel-labs/skills --skill find-skills');
    expect(detail.detailUrl).toBe('https://skills.sh/vercel-labs/skills/find-skills');
    expect(detail.repoUrl).toBe('https://github.com/vercel-labs/skills');
    expect(detail.source).toBe('vercel-labs/skills');
    expect(detail.stars).toBe(12800);
    expect(detail.weeklyInstalls).toBe(847500);
    expect(detail.firstSeen).toBe('Jan 26, 2026');
    expect(detail.securityAudits).toEqual([
      {
        name: 'Gen Agent Trust Hub',
        status: 'pass',
        url: 'https://skills.sh/vercel-labs/skills/find-skills/security/agent-trust-hub',
      },
      {
        name: 'Socket',
        status: 'pass',
        url: 'https://skills.sh/vercel-labs/skills/find-skills/security/socket',
      },
      {
        name: 'Snyk',
        status: 'warn',
        url: 'https://skills.sh/vercel-labs/skills/find-skills/security/snyk',
      },
    ]);
    expect(detail.installedOn).toEqual([
      {
        agent: 'opencode',
        installsText: '791.0K',
        installs: 791000,
      },
      {
        agent: 'codex',
        installsText: '787.5K',
        installs: 787500,
      },
    ]);
    expect(detail.highlights).toEqual([
      'Helps identify relevant skills by domain and task',
      'Presents install commands and links for user review',
    ]);
    expect(detail.contentHtml).toContain('<h2>Usage</h2>');
    expect(detail.contentHtml).toContain('Run it before selecting a new skill to install.');
    expect(detail.contentHtml).toContain('This list item belongs to the full SKILL.md');
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('caches skill detail requests', async () => {
    mockDiscoverFetch();

    const first = await getDiscoverSkillDetail('/vercel-labs/skills/find-skills');
    const second = await getDiscoverSkillDetail('/vercel-labs/skills/find-skills');

    expect(first.summary).toBe('Discover and install specialized agent skills from the open ecosystem.');
    expect(first.installCommand).toBe('npx skills add https://github.com/vercel-labs/skills --skill find-skills');
    expect(first.repoUrl).toBe('https://github.com/vercel-labs/skills');
    expect(first.stars).toBe(12800);
    expect(first.weeklyInstalls).toBe(847500);
    expect(first.summaryHtml).toContain('<ul>');
    expect(first.highlights).toEqual([
      'Helps identify relevant skills by domain and task',
      'Presents install commands and links for user review',
    ]);
    expect(first.contentHtml).toContain('<h2>Usage</h2>');
    expect(second).toEqual(first);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});

