/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { AgentSelector } from '../AgentSelector';
import type { AgentInfo } from '@/bindings';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

const agents: AgentInfo[] = [
  {
    id: 'codex',
    name: 'Codex',
    skillsDir: '.agents/skills',
    globalSkillsDir: '~/.codex/skills',
    detected: true,
    isUniversal: true,
    showInUniversalList: true,
  },
  {
    id: 'cursor',
    name: 'Cursor',
    skillsDir: '.agents/skills',
    globalSkillsDir: '~/.cursor/skills',
    detected: true,
    isUniversal: true,
    showInUniversalList: true,
  },
  {
    id: 'claude-code',
    name: 'Claude Code',
    skillsDir: '.claude/skills',
    globalSkillsDir: '~/.claude/skills',
    detected: true,
    isUniversal: false,
    showInUniversalList: false,
  },
  {
    id: 'windsurf',
    name: 'Windsurf',
    skillsDir: '.windsurf/skills',
    globalSkillsDir: '~/.codeium/windsurf/skills',
    detected: false,
    isUniversal: false,
    showInUniversalList: false,
  },
];

describe('AgentSelector', () => {
  it('presents the universal directory separately from additional agents', () => {
    render(
      <AgentSelector
        selectedAgents={[]}
        allAgents={agents}
        onSelectionChange={vi.fn()}
        scope="project"
      />
    );

    expect(screen.getByText('addSkill.agents.universalTitle')).toBeDefined();
    expect(screen.getByText('addSkill.agents.alwaysIncluded')).toBeDefined();
    expect(screen.getByText('./.agents/skills/')).toBeDefined();
    expect(screen.getByText('Codex')).toBeDefined();
    expect(screen.getByText('Cursor')).toBeDefined();
    expect(screen.getByText('addSkill.agents.additionalTitle')).toBeDefined();
    expect(screen.getByText('Claude Code')).toBeDefined();
    expect(screen.getByText('addSkill.agents.expandOtherAgents')).toBeDefined();
  });
});
