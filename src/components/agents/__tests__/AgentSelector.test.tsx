/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { AgentSelector } from '../AgentSelector';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { ResolvedAgent, ResolvedAgentScope } from '@/bindings';
import { makeResolvedScopeFixture, makeResolvedAgent } from '@/test-utils';

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
      if (key === 'addSkill.agents.configureUnknown') return 'Configure Agent';
      if (key === 'addSkill.agents.privateCopyTitle') {
        return 'Keep separately';
      }
      if (key === 'addSkill.agents.privateCopyHint') {
        return 'These Agents can already use the shared Skill directory. Select an Agent only when you also want to keep a link or copy in its own Skill directory; copied files may not update with the shared Skill.';
      }
      return key;
    },
  }),
}));

function makeAgent(agent: {
  id: string;
  name: string;
  skillsDir: string;
  globalSkillsDir: string;
  detected: boolean;
  globalAutomatic?: boolean;
  projectAutomatic?: boolean;
}): ResolvedAgent {
  return makeResolvedAgent({
    id: agent.id,
    displayName: agent.name,
    detection: agent.detected ? 'detected' : 'notDetected',
    global: makeResolvedScopeFixture({
      automatic: agent.globalAutomatic ?? false,
      path: agent.globalSkillsDir,
    }),
    project: makeResolvedScopeFixture({
      automatic: agent.projectAutomatic ?? false,
      path: agent.skillsDir,
      sharedPath: './.agents/skills',
    }),
  });
}

function withScopes(
  agent: ResolvedAgent,
  scopes: { global: ResolvedAgentScope; project: ResolvedAgentScope },
  detection = agent.detection,
): ResolvedAgent {
  return {
    ...agent,
    detection,
    global: scopes.global,
    project: scopes.project,
    definition: {
      ...agent.definition,
      global: {
        ...agent.definition.global,
        enabled: scopes.global.enabled,
        readsShared: scopes.global.readsShared,
      },
      project: {
        ...agent.definition.project,
        enabled: scopes.project.enabled,
        readsShared: scopes.project.readsShared,
      },
    },
  };
}

const agents: ResolvedAgent[] = [
  makeAgent({
    id: 'codex',
    name: 'Codex',
    skillsDir: '.agents/skills',
    globalSkillsDir: '~/.codex/skills',
    detected: true,
    projectAutomatic: true,
  }),
  makeAgent({
    id: 'cursor',
    name: 'Cursor',
    skillsDir: '.agents/skills',
    globalSkillsDir: '~/.cursor/skills',
    detected: true,
    projectAutomatic: true,
  }),
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

  it('shows an undetected user-defined Agent without requiring secondary expansion', () => {
    const customAgent = makeResolvedAgent({
      id: 'my-custom-agent',
      displayName: 'My Custom Agent',
      source: 'custom',
      detection: 'notDetected',
      global: {
        readsShared: true,
        sharedPath: '~/.agents/skills',
        privatePath: '~/.my-custom-agent/skills',
        readPaths: ['~/.agents/skills', '~/.my-custom-agent/skills'],
      },
      project: {
        readsShared: true,
        sharedPath: './.agents/skills',
        privatePath: './.my-custom-agent/skills',
        readPaths: ['./.agents/skills', './.my-custom-agent/skills'],
      },
    });

    renderAgentSelector(
      <AgentSelector
        selectedAgents={[]}
        privateCopyAgents={[]}
        allAgents={[customAgent]}
        onSelectionChange={vi.fn()}
        onPrivateCopyChange={vi.fn()}
        scope="global"
        privateCopyAgentsExpanded
      />
    );

    expect(screen.getAllByText('My Custom Agent')).toHaveLength(2);
    expect(screen.getByRole('checkbox', { name: /My Custom Agent/ })).toBeDefined();
    expect(screen.getByText('~/.my-custom-agent/skills')).toBeDefined();
  });

  it('keeps an indeterminate private-only Custom Agent selectable without calling it not detected', () => {
    const customAgent = makeResolvedAgent({
      id: 'private-custom-agent',
      displayName: 'Private Custom Agent',
      source: 'custom',
      detection: 'indeterminate',
      global: {
        readsShared: false,
        sharedPath: '~/.agents/skills',
        privatePath: '~/.private-custom-agent/skills',
        readPaths: ['~/.private-custom-agent/skills'],
      },
    });

    renderAgentSelector(
      <AgentSelector
        selectedAgents={[]}
        allAgents={[customAgent]}
        onSelectionChange={vi.fn()}
        scope="global"
      />
    );

    expect(screen.getByRole('checkbox', { name: /Private Custom Agent/ })).toBeDefined();
    expect(screen.getByText('addSkill.agents.indeterminate')).toBeDefined();
    expect(screen.queryByText('addSkill.agents.notDetected')).toBeNull();
  });

  it('shows optional agent-directory entries for eligible ready-to-use agents', () => {
    const onPrivateCopyChange = vi.fn();
    renderAgentSelector(
      <AgentSelector
        selectedAgents={[]}
        privateCopyAgents={[]}
        allAgents={[
          withScopes(agents[0], {
              global: makeResolvedScopeFixture({
                automatic: true,
                path: '~/.agents/skills',
                availability: 'shared-compatible',
                privatePath: '~/.codex/skills',
              }),
              project: makeResolvedScopeFixture({
                automatic: true,
                path: '.agents/skills',
                sharedPath: './.agents/skills',
              }),
            },
          ),
        ]}
        onSelectionChange={vi.fn()}
        onPrivateCopyChange={onPrivateCopyChange}
        scope="global"
        privateCopyAgentsExpanded
      />
    );

    expect(screen.getByText('Keep separately')).toBeDefined();
    expect(screen.getByText('These Agents can already use the shared Skill directory. Select an Agent only when you also want to keep a link or copy in its own Skill directory; copied files may not update with the shared Skill.')).toBeDefined();
    expect(screen.getByText('~/.codex/skills')).toBeDefined();
  });

  it('keeps optional agent-directory entries collapsed until requested when nothing is selected', () => {
    renderAgentSelector(
      <AgentSelector
        selectedAgents={[]}
        privateCopyAgents={[]}
        allAgents={[
          withScopes(agents[0], {
              global: makeResolvedScopeFixture({
                automatic: true,
                path: '~/.agents/skills',
                availability: 'shared-compatible',
                privatePath: '~/.codex/skills',
              }),
              project: makeResolvedScopeFixture({
                automatic: true,
                path: '.agents/skills',
                sharedPath: './.agents/skills',
              }),
            },
          ),
        ]}
        onSelectionChange={vi.fn()}
        onPrivateCopyChange={vi.fn()}
        scope="global"
      />
    );

    expect(screen.getByText('Keep separately')).toBeDefined();
    expect(screen.queryByText('~/.codex/skills')).toBeNull();
  });

  it('groups undetected optional agent-directory entries behind the same collapsed affordance', async () => {
    const { userEvent } = await import('@testing-library/user-event');
    const detectedAgent = withScopes(agents[0], {
        global: makeResolvedScopeFixture({
          automatic: true,
          path: '~/.agents/skills',
          availability: 'shared-compatible',
          privatePath: '~/.codex/skills',
        }),
        project: makeResolvedScopeFixture({
          automatic: true,
          path: '.agents/skills',
          sharedPath: './.agents/skills',
        }),
      },
    );
    const undetectedAgent = withScopes(agents[1], {
        global: makeResolvedScopeFixture({
          automatic: true,
          path: '~/.agents/skills',
          availability: 'shared-compatible',
          privatePath: '~/.cursor/skills',
        }),
        project: makeResolvedScopeFixture({
          automatic: true,
          path: '.agents/skills',
          sharedPath: './.agents/skills',
        }),
      },
    'notDetected');

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

  it('keeps selected undetected agent-directory entries visible for cleanup', () => {
    const undetectedAgent = withScopes(agents[1], {
        global: makeResolvedScopeFixture({
          automatic: true,
          path: '~/.agents/skills',
          availability: 'shared-compatible',
          privatePath: '~/.cursor/skills',
        }),
        project: makeResolvedScopeFixture({
          automatic: true,
          path: '.agents/skills',
          sharedPath: './.agents/skills',
        }),
      },
    'notDetected');

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

  it('does not prefix absolute kept agent-directory paths in project scope', () => {
    renderAgentSelector(
      <AgentSelector
        selectedAgents={[]}
        privateCopyAgents={[]}
        allAgents={[
          withScopes(agents[0], {
              global: makeResolvedScopeFixture({
                automatic: false,
                path: '~/.codex/skills',
              }),
              project: makeResolvedScopeFixture({
                automatic: true,
                path: '.agents/skills',
                sharedPath: './.agents/skills',
                availability: 'shared-compatible',
                privatePath: '/tmp/project/.codex/skills',
              }),
            },
          ),
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

  it('prefixes relative kept agent-directory paths in project scope', () => {
    renderAgentSelector(
      <AgentSelector
        selectedAgents={[]}
        privateCopyAgents={[]}
        allAgents={[
          withScopes(agents[0], {
              global: makeResolvedScopeFixture({
                automatic: false,
                path: '~/.codex/skills',
              }),
              project: makeResolvedScopeFixture({
                automatic: true,
                path: '.agents/skills',
                sharedPath: './.agents/skills',
                availability: 'shared-compatible',
                privatePath: '.codex/skills',
              }),
            },
          ),
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

  it('keeps unknown pasted Agent IDs visible with a configure action', () => {
    const onConfigureAgent = vi.fn();
    renderAgentSelector(
      <AgentSelector
        selectedAgents={[]}
        allAgents={[]}
        unknownAgentIds={['private-agent']}
        onSelectionChange={vi.fn()}
        onConfigureAgent={onConfigureAgent}
        scope="global"
      />
    );

    expect(screen.getByText('private-agent')).toBeDefined();
    fireEvent.click(screen.getByRole('button', { name: 'Configure Agent' }));
    expect(onConfigureAgent).toHaveBeenCalledWith('private-agent');
  });

  it('uses native checkbox semantics for selectable Agent rows', () => {
    renderAgentSelector(
      <AgentSelector
        selectedAgents={[]}
        allAgents={agents}
        onSelectionChange={vi.fn()}
        scope="global"
      />
    );

    expect(screen.getByRole('checkbox', { name: /Claude Code/ })).toBeDefined();
    expect(screen.queryByRole('button', { name: /Claude Code/ })).toBeNull();
  });

  it('renders one checkbox for Agents in the same Backend selection group', () => {
    const onSelectionChange = vi.fn();
    renderAgentSelector(
      <AgentSelector
        selectedAgents={['claude-code']}
        allAgents={[agents[2], agents[3]]}
        selectionGroups={[
          { groupId: 'opaque-group', agentIds: ['claude-code', 'windsurf'] },
        ]}
        onSelectionChange={onSelectionChange}
        scope="global"
      />
    );

    const checkbox = screen.getByRole('checkbox', { name: /Claude Code.*Windsurf/i });
    expect(screen.getAllByRole('checkbox')).toHaveLength(1);
    expect(checkbox.getAttribute('aria-checked')).toBe('true');

    fireEvent.click(checkbox);

    expect(onSelectionChange).toHaveBeenCalledWith([]);
  });
});
