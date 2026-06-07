/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { AgentSelector } from '../AgentSelector';
import type { AgentInfo } from '@/bindings';
import { makeAgentScopeTarget } from '@/test-utils';

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
      if (key === 'addSkill.agents.defaultAvailableHint') {
        return `Default hint: ${typeof options === 'object' ? options.path ?? '' : ''}`;
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
      global: makeAgentScopeTarget({
        automatic: agent.globalAutomatic ?? false,
        path: agent.globalSkillsDir,
      }),
      project: makeAgentScopeTarget({
        automatic: agent.projectAutomatic ?? false,
        path: agent.skillsDir,
        sharedPath: './.agents/skills',
      }),
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
      global: makeAgentScopeTarget({
        automatic: false,
        path: '~/.codex/skills',
      }),
      project: makeAgentScopeTarget({
        automatic: true,
        path: '.agents/skills',
        sharedPath: './.agents/skills',
      }),
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
      global: makeAgentScopeTarget({
        automatic: false,
        path: '~/.cursor/skills',
      }),
      project: makeAgentScopeTarget({
        automatic: true,
        path: '.agents/skills',
        sharedPath: './.agents/skills',
      }),
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
  it('shows default available and separate setup groups for the selected scope', () => {
    render(
      <AgentSelector
        selectedAgents={[]}
        allAgents={agents}
        onSelectionChange={vi.fn()}
        scope="project"
      />
    );

    expect(screen.getByText('addSkill.agents.defaultAvailableTitle')).toBeDefined();
    expect(screen.getByText('Default hint: ./.agents/skills')).toBeDefined();
    expect(screen.getByText('Codex')).toBeDefined();
    expect(screen.getByText('Cursor')).toBeDefined();
    expect(screen.getByText('addSkill.agents.privateRequiredTitle')).toBeDefined();
    expect(screen.getByText('Claude Code')).toBeDefined();
    expect(screen.getByText('Show 1 more agents')).toBeDefined();
  });

  it('shows advanced private copy options for eligible default-available agents', () => {
    const onPrivateCopyChange = vi.fn();
    render(
      <AgentSelector
        selectedAgents={[]}
        privateCopyAgents={[]}
        allAgents={[
          {
            ...agents[0],
            targets: {
              global: makeAgentScopeTarget({
                automatic: true,
                path: '~/.agents/skills',
                availability: 'shared-compatible',
                privatePath: '~/.codex/skills',
              }),
              project: makeAgentScopeTarget({
                automatic: true,
                path: '.agents/skills',
                sharedPath: './.agents/skills',
              }),
            },
          },
        ]}
        onSelectionChange={vi.fn()}
        onPrivateCopyChange={onPrivateCopyChange}
        scope="global"
        privateCopyAgentsExpanded
      />
    );

    expect(screen.getByText('addSkill.agents.privateCopyTitle')).toBeDefined();
    expect(screen.getByText('~/.codex/skills')).toBeDefined();
  });

  it('does not prefix absolute private copy paths in project scope', () => {
    render(
      <AgentSelector
        selectedAgents={[]}
        privateCopyAgents={[]}
        allAgents={[
          {
            ...agents[0],
            targets: {
              global: makeAgentScopeTarget({
                automatic: false,
                path: '~/.codex/skills',
              }),
              project: makeAgentScopeTarget({
                automatic: true,
                path: '.agents/skills',
                sharedPath: './.agents/skills',
                availability: 'shared-compatible',
                privatePath: '/tmp/project/.codex/skills',
              }),
            },
          },
        ]}
        onSelectionChange={vi.fn()}
        onPrivateCopyChange={vi.fn()}
        scope="project"
        privateCopyAgentsExpanded
      />
    );

    expect(screen.getByText('/tmp/project/.codex/skills')).toBeDefined();
    expect(screen.queryByText('./tmp/project/.codex/skills')).toBeNull();
  });

  it('prefixes relative private copy paths in project scope', () => {
    render(
      <AgentSelector
        selectedAgents={[]}
        privateCopyAgents={[]}
        allAgents={[
          {
            ...agents[0],
            targets: {
              global: makeAgentScopeTarget({
                automatic: false,
                path: '~/.codex/skills',
              }),
              project: makeAgentScopeTarget({
                automatic: true,
                path: '.agents/skills',
                sharedPath: './.agents/skills',
                availability: 'shared-compatible',
                privatePath: '.codex/skills',
              }),
            },
          },
        ]}
        onSelectionChange={vi.fn()}
        onPrivateCopyChange={vi.fn()}
        scope="project"
        privateCopyAgentsExpanded
      />
    );

    expect(screen.getByText('./.codex/skills')).toBeDefined();
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
