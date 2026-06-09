/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { AgentSelector } from '../AgentSelector';
import { TooltipProvider } from '@/components/ui/tooltip';
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
        return `These Agents are ready to use after install. No selection is needed. ${typeof options === 'object' ? options.path ?? '' : ''}`;
      }
      if (key === 'addSkill.agents.privateRequiredHint') {
        return 'These Agents need to be connected to the Skill separately. When selected, install will create a link or copy for them.';
      }
      if (key === 'addSkill.agents.privateCopyTitle') {
        return 'Dedicated copies';
      }
      if (key === 'addSkill.agents.privateCopyHint') {
        return 'These Agents can use the shared Skill directory. Select an Agent only when you want it to keep its own copy; copies may not update with the shared Skill.';
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

function renderAgentSelector(ui: React.ReactElement) {
  return render(<TooltipProvider>{ui}</TooltipProvider>);
}

describe('AgentSelector', () => {
  it('shows default available and separate setup groups for the selected scope', () => {
    renderAgentSelector(
      <AgentSelector
        selectedAgents={[]}
        allAgents={agents}
        onSelectionChange={vi.fn()}
        scope="project"
      />
    );

    expect(screen.getByText('addSkill.agents.defaultAvailableTitle')).toBeDefined();
    expect(screen.getByText('These Agents are ready to use after install. No selection is needed. ./.agents/skills')).toBeDefined();
    expect(screen.getByText('Codex')).toBeDefined();
    expect(screen.getByText('Cursor')).toBeDefined();
    expect(screen.getByText('addSkill.agents.privateRequiredTitle')).toBeDefined();
    expect(screen.getByText('These Agents need to be connected to the Skill separately. When selected, install will create a link or copy for them.')).toBeDefined();
    expect(screen.getByText('Claude Code')).toBeDefined();
    expect(screen.getByText('Show 1 more agents')).toBeDefined();
  });

  it('shows separate version options for eligible ready-to-use agents', () => {
    const onPrivateCopyChange = vi.fn();
    renderAgentSelector(
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

    expect(screen.getByText('Dedicated copies')).toBeDefined();
    expect(screen.getByText('These Agents can use the shared Skill directory. Select an Agent only when you want it to keep its own copy; copies may not update with the shared Skill.')).toBeDefined();
    expect(screen.getByText('~/.codex/skills')).toBeDefined();
  });

  it('keeps dedicated copy options collapsed until requested when nothing is selected', () => {
    renderAgentSelector(
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
        onPrivateCopyChange={vi.fn()}
        scope="global"
      />
    );

    expect(screen.getByText('Dedicated copies')).toBeDefined();
    expect(screen.queryByText('~/.codex/skills')).toBeNull();
  });

  it('groups undetected dedicated copy options behind the same collapsed affordance', async () => {
    const { userEvent } = await import('@testing-library/user-event');
    const detectedAgent: AgentInfo = {
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
    };
    const undetectedAgent: AgentInfo = {
      ...agents[1],
      detected: false,
      targets: {
        global: makeAgentScopeTarget({
          automatic: true,
          path: '~/.agents/skills',
          availability: 'shared-compatible',
          privatePath: '~/.cursor/skills',
        }),
        project: makeAgentScopeTarget({
          automatic: true,
          path: '.agents/skills',
          sharedPath: './.agents/skills',
        }),
      },
    };

    renderAgentSelector(
      <AgentSelector
        selectedAgents={[]}
        privateCopyAgents={[]}
        allAgents={[detectedAgent, undetectedAgent]}
        onSelectionChange={vi.fn()}
        onPrivateCopyChange={vi.fn()}
        scope="global"
        privateCopyAgentsExpanded
      />
    );

    expect(screen.getAllByText('Codex').length).toBeGreaterThan(1);
    expect(screen.queryByText('Cursor')).toBeNull();

    const expandButton = screen.getByRole('button', { name: /Show 1 more agents/i });
    await userEvent.click(expandButton);

    expect(screen.getByText('Cursor')).toBeDefined();
  });

  it('keeps selected undetected dedicated copy options visible for cleanup', () => {
    const undetectedAgent: AgentInfo = {
      ...agents[1],
      detected: false,
      targets: {
        global: makeAgentScopeTarget({
          automatic: true,
          path: '~/.agents/skills',
          availability: 'shared-compatible',
          privatePath: '~/.cursor/skills',
        }),
        project: makeAgentScopeTarget({
          automatic: true,
          path: '.agents/skills',
          sharedPath: './.agents/skills',
        }),
      },
    };

    renderAgentSelector(
      <AgentSelector
        selectedAgents={[]}
        privateCopyAgents={['cursor']}
        allAgents={[undetectedAgent]}
        onSelectionChange={vi.fn()}
        onPrivateCopyChange={vi.fn()}
        scope="global"
      />
    );

    expect(screen.getByText('Cursor')).toBeDefined();
    expect(screen.getByText('~/.cursor/skills')).toBeDefined();
  });

  it('does not prefix absolute private copy paths in project scope', () => {
    renderAgentSelector(
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
    renderAgentSelector(
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
    renderAgentSelector(
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
    renderAgentSelector(
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

  it('constrains long path rows so they can shrink inside dialogs', () => {
    renderAgentSelector(
      <AgentSelector
        selectedAgents={[]}
        allAgents={[
          makeAgent({
            id: 'claude-code',
            name: 'Claude Code',
            skillsDir: '.claude/skills',
            globalSkillsDir: '/Users/example/projects/very/long/path/that/should/not/push/dialog/width/.claude/skills',
            detected: true,
          }),
        ]}
        onSelectionChange={vi.fn()}
        scope="global"
      />
    );

    const path = screen.getByText('/Users/example/projects/very/long/path/that/should/not/push/dialog/width/.claude/skills');
    expect(path.className).toContain('truncate');
    expect(path.parentElement?.className).toContain('min-w-0');
    expect(path.parentElement?.className).toContain('flex-1');
  });
});
