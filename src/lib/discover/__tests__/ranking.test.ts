import { describe, expect, it } from 'vitest';
import type { DiscoverSkillSummary } from '../types';
import {
  extractSourceOwner,
  filterDiscoverSkills,
  parseMetric,
  sortDiscoverSkills,
} from '../ranking';

function makeSkill(overrides: Partial<DiscoverSkillSummary>): DiscoverSkillSummary {
  return {
    slug: 'demo',
    name: 'Demo',
    source: 'owner/demo',
    summary: 'Demo skill',
    installs: 100,
    displayMetric: {
      kind: 'installs',
      rawText: '100',
      sortValue: 100,
    },
    relevanceScore: 10,
    isOfficial: false,
    detailUrl: 'https://skills.sh/owner/demo',
    ...overrides,
  };
}

describe('discover ranking utilities', () => {
  it('parses installs shorthand metrics', () => {
    expect(parseMetric('651.3K')).toBe(651300);
    expect(parseMetric('2.5M')).toBe(2500000);
    expect(parseMetric('42')).toBe(42);
  });

  it('extracts source owner from repo forms and github urls', () => {
    expect(extractSourceOwner('vercel-labs/skills')).toBe('vercel-labs');
    expect(extractSourceOwner('https://github.com/vercel-labs/skills')).toBe('vercel-labs');
    expect(extractSourceOwner('git@github.com:vercel-labs/skills.git')).toBe('vercel-labs');
  });

  it('keeps search relevance order first and only breaks ties with installs and official', () => {
    const skills = [
      makeSkill({ slug: 'top', relevanceScore: 100, installs: 10, isOfficial: false }),
      makeSkill({ slug: 'tie-installed', relevanceScore: 50, installs: 999, isOfficial: false }),
      makeSkill({ slug: 'tie-official', relevanceScore: 50, installs: 100, isOfficial: true }),
      makeSkill({ slug: 'tie-plain', relevanceScore: 50, installs: 100, isOfficial: false }),
    ];

    const sorted = sortDiscoverSkills(skills, { mode: 'search', sort: 'best-match' });

    expect(sorted.map((skill) => skill.slug)).toEqual(['top', 'tie-installed', 'tie-official', 'tie-plain']);
    expect(skills.map((skill) => skill.slug)).toEqual(['top', 'tie-installed', 'tie-official', 'tie-plain']);
  });

  it('sorts browse results by installs descending and official first on ties', () => {
    const skills = [
      makeSkill({
        slug: 'b',
        name: 'B',
        installs: 20,
        displayMetric: { kind: 'installs', rawText: '20', sortValue: 20 },
        isOfficial: false,
      }),
      makeSkill({
        slug: 'c',
        name: 'C',
        installs: 100,
        displayMetric: { kind: 'installs', rawText: '100', sortValue: 100 },
        isOfficial: false,
      }),
      makeSkill({
        slug: 'a',
        name: 'A',
        installs: 100,
        displayMetric: { kind: 'installs', rawText: '100', sortValue: 100 },
        isOfficial: true,
      }),
    ];

    const sorted = sortDiscoverSkills(skills, { mode: 'browse', sort: 'installs' });

    expect(sorted.map((skill) => skill.slug)).toEqual(['a', 'c', 'b']);
    expect(skills.map((skill) => skill.slug)).toEqual(['b', 'c', 'a']);
  });

  it('sorts browse trending results by display metric instead of installs', () => {
    const skills = [
      makeSkill({
        slug: 'highest-trending',
        installs: 10,
        displayMetric: { kind: 'trending-24h', rawText: '13.9K', sortValue: 13900 },
      }),
      makeSkill({
        slug: 'official-tie',
        installs: 500,
        isOfficial: true,
        displayMetric: { kind: 'hot', rawText: '248 +248', sortValue: 248 },
      }),
      makeSkill({
        slug: 'plain-tie',
        installs: 900,
        isOfficial: false,
        displayMetric: { kind: 'hot', rawText: '248 +248', sortValue: 248 },
      }),
    ];

    const sorted = sortDiscoverSkills(skills, { mode: 'browse', sort: 'trending' });

    expect(sorted.map((skill) => skill.slug)).toEqual(['highest-trending', 'official-tie', 'plain-tie']);
    expect(skills.map((skill) => skill.slug)).toEqual(['highest-trending', 'official-tie', 'plain-tie']);
  });

  it('treats zero display metric as a valid trending sort value', () => {
    const skills = [
      makeSkill({
        slug: 'zero-trending',
        weeklyInstalls: 999,
        displayMetric: { kind: 'hot', rawText: '0 +0', sortValue: 0 },
      }),
      makeSkill({
        slug: 'non-zero-trending',
        weeklyInstalls: 1,
        displayMetric: { kind: 'hot', rawText: '1 +1', sortValue: 1 },
      }),
    ];

    const sorted = sortDiscoverSkills(skills, { mode: 'browse', sort: 'trending' });

    expect(sorted.map((skill) => skill.slug)).toEqual(['non-zero-trending', 'zero-trending']);
  });

  it('filters by official, not-installed, and risk', () => {
    const skills = [
      makeSkill({ slug: 'a', name: 'Alpha', source: 'owner/alpha', isOfficial: true, installs: 100, auditRisk: 'safe' }),
      makeSkill({ slug: 'b', name: 'Beta', source: 'owner/beta', isOfficial: false, installs: 50, auditRisk: 'medium' }),
      makeSkill({ slug: 'c', name: 'Gamma', source: 'owner/gamma', isOfficial: false, installs: 10, auditRisk: 'critical' }),
    ];
    const installedSkillKeys = new Set(['owner/alpha::Alpha']);

    expect(filterDiscoverSkills(skills, { officialOnly: true })).toHaveLength(1);
    expect(filterDiscoverSkills(skills, { notInstalledOnly: true, installedSkillKeys })).toHaveLength(2);
    expect(filterDiscoverSkills(skills, { risk: ['medium', 'critical'] }).map((skill) => skill.slug)).toEqual(['b', 'c']);
  });
});
