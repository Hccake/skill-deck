import { describe, expect, it } from 'vitest';
import { makeResolvedAgent } from '@/test-utils';
import {
  filterSkills,
  getAgentFilterOptions,
  getSkillAssociatedAgentIds,
} from '../filter';
import type { InstalledSkill } from '@/bindings';

function makeSkill(
  name: string,
  overrides: Partial<InstalledSkill> = {},
): InstalledSkill {
  return {
    name,
    description: `${name} description`,
    path: `/skills/${name}`,
    canonicalPath: `/canonical/${name}`,
    scope: 'global',
    agents: [],
    associatedAgents: [],
    ...overrides,
  };
}

describe('Skill filters', () => {
  it('uses the Backend associated Agent projection as the authority', () => {
    const skill = {
      ...makeSkill('toolkit', {
      agents: ['legacy-agent'],
      defaultAvailableAgents: ['claude-code'],
      }),
      associatedAgents: ['codex', 'codex'],
    } as unknown as InstalledSkill;

    expect(getSkillAssociatedAgentIds(skill)).toEqual(['codex']);
  });

  it('treats an empty associated Agent projection as authoritative', () => {
    const skill = makeSkill('empty', {
      agents: ['legacy-agent'],
      defaultAvailableAgents: ['codex'],
      privateAdaptedAgents: ['claude-code'],
      privateCopyAgents: ['qwen-code'],
      associatedAgents: [],
    });

    expect(getSkillAssociatedAgentIds(skill)).toEqual([]);
  });

  it('treats all as a regular Agent ID instead of a sentinel value', () => {
    const matched = makeSkill('matched', { associatedAgents: ['all'] });
    const unrelated = makeSkill('unrelated', { associatedAgents: ['codex'] });

    expect(filterSkills([matched, unrelated], '', 'all')).toEqual([matched]);
    expect(filterSkills([matched, unrelated], '', null)).toEqual([matched, unrelated]);
  });

  it('offers every detected scope Agent even when its Skill count is zero', () => {
    const codex = makeResolvedAgent({ id: 'codex', displayName: 'Codex' });
    const cursor = makeResolvedAgent({ id: 'cursor', displayName: 'Cursor' });
    const hidden = makeResolvedAgent({
      id: 'hidden',
      displayName: 'Hidden',
      detection: 'notDetected',
    });
    expect(getAgentFilterOptions([cursor, hidden, codex], null)).toEqual([codex, cursor]);
  });

  it('keeps the selected Agent option while detection is temporarily unavailable', () => {
    const codex = makeResolvedAgent({
      id: 'codex',
      displayName: 'Codex',
      detection: 'notDetected',
    });

    expect(getAgentFilterOptions([codex], 'codex')).toEqual([codex]);
    expect(getAgentFilterOptions([codex], null)).toEqual([]);
  });

  it('combines search and Agent conditions with AND semantics', () => {
    const writer = {
      ...makeSkill('writer'),
      associatedAgents: ['codex'],
    } as unknown as InstalledSkill;
    const reviewer = {
      ...makeSkill('reviewer'),
      associatedAgents: ['cursor'],
    } as unknown as InstalledSkill;

    expect(filterSkills([writer, reviewer], 'writ', 'codex')).toEqual([writer]);
    expect(filterSkills([writer, reviewer], 'review', 'codex')).toEqual([]);
  });
});
