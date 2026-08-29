import { describe, expect, it } from 'vitest';
import type { AvailableSkill } from '@/bindings';
import { resolveSourceSelection } from '../source-selection';

const skills: AvailableSkill[] = [
  {
    name: 'alpha',
    installDirName: 'alpha',
    description: 'Alpha',
    relativePath: 'skills/alpha/SKILL.md',
  },
  {
    name: 'beta',
    installDirName: 'beta',
    description: 'Beta',
    relativePath: 'skills/beta/SKILL.md',
  },
];

describe('resolveSourceSelection', () => {
  it.each([
    {
      name: 'selects a Skill addressed through @skill',
      input: 'owner/repo@alpha',
      skillFilter: 'alpha',
      expected: ['alpha'],
    },
    {
      name: 'selects explicit Skills from a CLI command',
      input: 'skills add owner/repo --skill beta',
      skillFilter: null,
      expected: ['beta'],
    },
    {
      name: 'selects every discovered Skill for a Pack',
      input: 'https://skills.sh/p/frontend',
      skillFilter: null,
      expected: ['alpha', 'beta'],
    },
    {
      name: 'selects every discovered Skill for a wildcard',
      input: 'skills add owner/repo --skill *',
      skillFilter: null,
      expected: ['alpha', 'beta'],
    },
    {
      name: 'leaves an ordinary repository unselected',
      input: 'owner/repo',
      skillFilter: null,
      expected: [],
    },
  ])('$name', ({ input, skillFilter, expected }) => {
    expect(resolveSourceSelection(input, { skills, skillFilter }).selectedSkillNames)
      .toEqual(expected);
  });

  it('normalizes CLI source and preserves source and Agent selection intent', () => {
    expect(resolveSourceSelection(
      'skills add owner/repo --skill alpha --agent codex --all',
      { skills, skillFilter: null },
    )).toEqual({
      source: 'owner/repo',
      selectedSkillNames: ['alpha', 'beta'],
      sourceSelectionIntent: {
        wildcardRequested: true,
        explicitSkillNames: ['alpha'],
      },
      agentSelectionIntent: {
        wildcardRequested: true,
        explicitAgentIds: ['codex'],
      },
    });
  });
});
