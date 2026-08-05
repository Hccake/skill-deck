import { describe, expect, it } from 'vitest';
import { getSharedSkillDirectory } from '../agentTargets';

describe('Agent target helpers', () => {
  it('provides normalized shared Skill directory display paths', () => {
    expect(getSharedSkillDirectory('global')).toBe('~/.agents/skills');
    expect(getSharedSkillDirectory('project')).toBe('./.agents/skills');
  });
});
