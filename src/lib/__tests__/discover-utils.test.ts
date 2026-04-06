import { describe, it, expect } from 'vitest';
import { getSkillInstallLocations, isSkillInstalled } from '../discover-utils';
import type { DiscoverSkillSummary } from '../discover/types';

function makeSkill(source: string, name: string): DiscoverSkillSummary {
  return {
    slug: name,
    name,
    source,
    displayMetric: { kind: 'installs', rawText: '100', sortValue: 100 },
    isOfficial: false,
    detailUrl: `https://skills.sh/${name}`,
  };
}

describe('getSkillInstallLocations', () => {
  it('returns empty array when skill is not installed anywhere', () => {
    const map = new Map<string, string[]>();
    const skill = makeSkill('owner/repo', 'my-skill');
    expect(getSkillInstallLocations(map, skill)).toEqual([]);
  });

  it('returns locations when matched by full URL source', () => {
    const map = new Map<string, string[]>([
      ['https://github.com/owner/repo::my-skill', ['global']],
    ]);
    const skill = makeSkill('https://github.com/owner/repo', 'my-skill');
    expect(getSkillInstallLocations(map, skill)).toEqual(['global']);
  });

  it('returns locations when matched by normalized short source', () => {
    const map = new Map<string, string[]>([
      ['owner/repo::my-skill', ['global', 'D:/projects/app-a']],
    ]);
    const skill = makeSkill('https://github.com/owner/repo', 'my-skill');
    expect(getSkillInstallLocations(map, skill)).toEqual(['global', 'D:/projects/app-a']);
  });

  it('prefers full source match over normalized match', () => {
    const map = new Map<string, string[]>([
      ['https://github.com/owner/repo::my-skill', ['global']],
      ['owner/repo::my-skill', ['D:/projects/app-a']],
    ]);
    const skill = makeSkill('https://github.com/owner/repo', 'my-skill');
    // Full match takes priority
    expect(getSkillInstallLocations(map, skill)).toEqual(['global']);
  });

  it('handles skill with short source matching short key', () => {
    const map = new Map<string, string[]>([
      ['owner/repo::my-skill', ['global']],
    ]);
    const skill = makeSkill('owner/repo', 'my-skill');
    expect(getSkillInstallLocations(map, skill)).toEqual(['global']);
  });

  it('returns empty when skill has short source but map only has full URL key', () => {
    const map = new Map<string, string[]>([
      ['https://github.com/owner/repo::my-skill', ['global']],
    ]);
    const skill = makeSkill('owner/repo', 'my-skill');
    // normalizedSource = 'owner/repo', both lookups try 'owner/repo::my-skill' — no match
    expect(getSkillInstallLocations(map, skill)).toEqual([]);
  });
});

describe('isSkillInstalled (convenience wrapper)', () => {
  it('returns true when skill has install locations', () => {
    const map = new Map<string, string[]>([
      ['owner/repo::my-skill', ['global']],
    ]);
    const skill = makeSkill('owner/repo', 'my-skill');
    expect(isSkillInstalled(map, skill)).toBe(true);
  });

  it('returns false when skill has no install locations', () => {
    const map = new Map<string, string[]>();
    const skill = makeSkill('owner/repo', 'my-skill');
    expect(isSkillInstalled(map, skill)).toBe(false);
  });
});
