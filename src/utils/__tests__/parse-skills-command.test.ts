import { describe, expect, it } from 'vitest';
import { isSkillsShPackUrl, parseSkillsCommand } from '../parse-skills-command';

describe('parseSkillsCommand', () => {
  it('keeps ordinary source input unchanged apart from surrounding whitespace', () => {
    expect(parseSkillsCommand('  owner/repo@reviewer  ')).toEqual({
      source: 'owner/repo@reviewer',
      skills: [],
      agents: [],
      isCommand: false,
    });
  });

  it.each(['add', 'install', 'a', 'i'])(
    'recognizes the skills %s command',
    (command) => {
      expect(parseSkillsCommand(`skills ${command} owner/repo`)).toEqual({
        source: 'owner/repo',
        skills: [],
        agents: [],
        isCommand: true,
      });
    },
  );

  it('recognizes the npx command prefix and ignores boolean installation flags', () => {
    expect(parseSkillsCommand(
      'npx skills add owner/repo --global --yes --all --list',
    )).toEqual({
      source: 'owner/repo',
      skills: [],
      agents: [],
      isCommand: true,
    });
  });

  it('collects repeated long and short Skill and Agent selectors', () => {
    expect(parseSkillsCommand(
      'skills add owner/repo --skill reviewer -s writer --agent codex -a claude-code',
    )).toEqual({
      source: 'owner/repo',
      skills: ['reviewer', 'writer'],
      agents: ['codex', 'claude-code'],
      isCommand: true,
    });
  });

  it('keeps quoted selector values together', () => {
    expect(parseSkillsCommand(
      'skills add owner/repo --skill "Convex Best Practices" --agent \'custom agent\'',
    )).toEqual({
      source: 'owner/repo',
      skills: ['Convex Best Practices'],
      agents: ['custom agent'],
      isCommand: true,
    });
  });

  it('does not turn wildcard selectors into explicit preselection', () => {
    expect(parseSkillsCommand(
      "skills add owner/repo --skill '*' --agent '*'",
    )).toEqual({
      source: 'owner/repo',
      skills: [],
      agents: [],
      isCommand: true,
    });
  });

  it('returns an empty source when a recognized command only contains selectors', () => {
    expect(parseSkillsCommand('skills add --skill reviewer')).toEqual({
      source: '',
      skills: ['reviewer'],
      agents: [],
      isCommand: true,
    });
  });

  it('ignores a selector that has no following value', () => {
    expect(parseSkillsCommand('skills install owner/repo --agent')).toEqual({
      source: 'owner/repo',
      skills: [],
      agents: [],
      isCommand: true,
    });
  });
});

describe('isSkillsShPackUrl', () => {
  it.each([
    ['https://skills.sh/p/frontend', true],
    ['https://www.skills.sh/p/frontend', true],
    ['https://skills.sh/p/', false],
    ['https://skills.sh/acme/review', false],
    ['https://example.com/p/frontend', false],
    ['owner/repo', false],
  ])('classifies %s as %s', (source, expected) => {
    expect(isSkillsShPackUrl(source)).toBe(expected);
  });
});
