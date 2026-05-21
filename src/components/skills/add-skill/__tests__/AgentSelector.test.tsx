/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { AgentSelector } from '../AgentSelector';
import type { AgentInfo } from '@/bindings';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: string | { count?: number; path?: string }) => {
      if (key === 'addSkill.agents.expandOtherAgents') {
        return `Show ${typeof options === 'object' ? options.count ?? 0 : 0} more agents`;
      }
      if (key === 'addSkill.agents.collapseOtherAgents') {
        if (typeof options === 'string') return options;
        return 'Collapse options';
      }
      if (key === 'addSkill.agents.automaticHint') {
        return `Automatic hint: ${typeof options === 'object' ? options.path ?? '' : ''}`;
      }
      return key;
    },
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
    expect(screen.getByText('Automatic hint: ./.agents/skills')).toBeDefined();
    expect(screen.getByText('Codex')).toBeDefined();
    expect(screen.getByText('Cursor')).toBeDefined();
    expect(screen.getByText('addSkill.agents.additionalTitle')).toBeDefined();
    expect(screen.getByText('Claude Code')).toBeDefined();
    expect(screen.getByText('Show 1 more agents')).toBeDefined();
  });

  it('keeps expand and collapse labels free of arrow glyphs', async () => {
    const { userEvent } = await import('@testing-library/user-event');
    render(
      <AgentSelector
        selectedAgents={[]}
        allAgents={agents}
        onSelectionChange={vi.fn()}
        scope="project"
      />
    );

    const expandButton = screen.getByRole('button', { name: /Show 1 more agents/i });
    expect(expandButton.textContent).not.toMatch(/[↓↑∧]/);

    await userEvent.click(expandButton);

    const collapseButton = screen.getByRole('button', { name: /Collapse options/i });
    expect(collapseButton.textContent).not.toMatch(/[↓↑∧]/);
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
