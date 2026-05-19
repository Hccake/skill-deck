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

function makeAgent(agent: Omit<AgentInfo, 'targets'> & {
  globalAutomatic?: boolean;
  projectAutomatic?: boolean;
}): AgentInfo {
  return {
    ...agent,
    targets: {
      global: {
        supported: true,
        automatic: agent.globalAutomatic ?? false,
        path: agent.globalSkillsDir,
      },
      project: {
        supported: true,
        automatic: agent.projectAutomatic ?? false,
        path: agent.skillsDir,
      },
    },
  };
}

const agents: AgentInfo[] = [
  {
    ...makeAgent({
    id: 'codex',
    name: 'Codex',
    skillsDir: '.agents/skills',
    globalSkillsDir: '~/.codex/skills',
    detected: true,
    projectAutomatic: true,
    }),
    targets: {
      global: {
        supported: true,
        automatic: false,
        path: '~/.codex/skills',
      },
      project: {
        supported: true,
        automatic: true,
        path: '.agents/skills',
      },
    },
  },
  {
    ...makeAgent({
    id: 'cursor',
    name: 'Cursor',
    skillsDir: '.agents/skills',
    globalSkillsDir: '~/.cursor/skills',
    detected: true,
    projectAutomatic: true,
    }),
    targets: {
      global: {
        supported: true,
        automatic: false,
        path: '~/.cursor/skills',
      },
      project: {
        supported: true,
        automatic: true,
        path: '.agents/skills',
      },
    },
  },
  makeAgent({
    id: 'claude-code',
    name: 'Claude Code',
    skillsDir: '.claude/skills',
    globalSkillsDir: '~/.claude/skills',
    detected: true,
  }),
  makeAgent({
    id: 'windsurf',
    name: 'Windsurf',
    skillsDir: '.windsurf/skills',
    globalSkillsDir: '~/.codeium/windsurf/skills',
    detected: false,
  }),
];

describe('AgentSelector', () => {
  it('presents automatic and manual targets for the selected scope', () => {
    render(
      <AgentSelector
        selectedAgents={[]}
        allAgents={agents}
        onSelectionChange={vi.fn()}
        scope="project"
      />
    );

    expect(screen.getByText('addSkill.agents.automaticTitle')).toBeDefined();
    expect(screen.getByText('./.agents/skills/')).toBeDefined();
    expect(screen.getByText('Codex')).toBeDefined();
    expect(screen.getByText('Cursor')).toBeDefined();
    expect(screen.getByText('addSkill.agents.additionalTitle')).toBeDefined();
    expect(screen.getByText('Claude Code')).toBeDefined();
    expect(screen.getByText('addSkill.agents.expandOtherAgents')).toBeDefined();
  });

  it('uses global target metadata when global scope is selected', () => {
    render(
      <AgentSelector
        selectedAgents={[]}
        allAgents={agents}
        onSelectionChange={vi.fn()}
        scope="global"
      />
    );

    expect(screen.getByText('Codex')).toBeDefined();
    expect(screen.getByText('~/.codex/skills')).toBeDefined();
    expect(screen.getByText('Cursor')).toBeDefined();
    expect(screen.getByText('~/.cursor/skills')).toBeDefined();
    expect(screen.getByText('Claude Code')).toBeDefined();
    expect(screen.getByText('~/.claude/skills')).toBeDefined();
  });
});
