/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { ManageAgentsDialog } from '../ManageAgentsDialog';
import type { AgentInfo, InstalledSkill, SkillAgentDetails } from '@/bindings';
import { makeAgentScopeTarget } from '@/test-utils';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) =>
      values?.name ? `${key}:${values.name}` : key,
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

const allAgents: AgentInfo[] = [
  makeAgent({
    id: 'claude-code',
    name: 'Claude Code',
    skillsDir: '.claude/skills',
    globalSkillsDir: '~/.claude/skills',
    detected: true,
  }),
  makeAgent({
    id: 'cursor',
    name: 'Cursor',
    skillsDir: '.cursor/skills',
    globalSkillsDir: '~/.cursor/skills',
    detected: true,
  }),
];

const skill: InstalledSkill = {
  name: 'agent-toolkit',
  description: 'Agent toolkit',
  path: '/skills/agent-toolkit',
  canonicalPath: '/canonical/agent-toolkit',
  scope: 'project',
  agents: ['claude-code'],
};

describe('ManageAgentsDialog', () => {
  it('resets selected separate locations when agent metadata changes', () => {
    const automaticCursor: AgentInfo = makeAgent({
      id: 'cursor',
      name: 'Cursor',
      skillsDir: '.agents/skills',
      globalSkillsDir: '~/.cursor/skills',
      detected: true,
      projectAutomatic: true,
    });
    const skillWithCursor: InstalledSkill = {
      ...skill,
      agents: ['cursor'],
    };

    const { rerender } = render(
      <ManageAgentsDialog
        skill={skillWithCursor}
        scope="project"
        allAgents={[]}
        onClose={vi.fn()}
        onSave={vi.fn()}
      />
    );

    rerender(
      <ManageAgentsDialog
        skill={skillWithCursor}
        scope="project"
        allAgents={[automaticCursor]}
        onClose={vi.fn()}
        onSave={vi.fn()}
      />
    );

    expect(screen.queryByText('skills.manageAgents.modeTitle')).toBeNull();
    const saveButton = screen.getByRole('button', {
      name: 'skills.manageAgents.save',
    }) as HTMLButtonElement;
    expect(saveButton.disabled).toBe(true);
  });

  it('hides the install method until a separate agent location is added', () => {
    render(
      <ManageAgentsDialog
        skill={skill}
        scope="project"
        allAgents={allAgents}
        onClose={vi.fn()}
        onSave={vi.fn()}
      />
    );

    expect(screen.queryByText('skills.manageAgents.modeTitle')).toBeNull();
  });

  it('passes selected mode when saving newly added agents', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn().mockResolvedValue(undefined);

    render(
      <ManageAgentsDialog
        skill={skill}
        scope="project"
        allAgents={allAgents}
        onClose={vi.fn()}
        onSave={onSave}
      />
    );

    await user.click(screen.getByText('Cursor'));
    await user.click(screen.getByText('addSkill.mode.copy'));
    await user.click(screen.getByRole('button', { name: 'skills.manageAgents.save' }));

    expect(onSave).toHaveBeenCalledWith(['cursor'], [], 'copy', [], []);
  });

  it('shows existing duplicate copies as selected dedicated copies and saves deselection', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn().mockResolvedValue(undefined);
    const sharedCompatibleAgent: AgentInfo = {
      id: 'firebender',
      name: 'Firebender',
      skillsDir: '.firebender/skills',
      globalSkillsDir: '~/.firebender/skills',
      detected: true,
      targets: {
        global: makeAgentScopeTarget({
          automatic: true,
          path: '~/.agents/skills',
          availability: 'shared-compatible',
          privatePath: '~/.firebender/skills',
        }),
        project: makeAgentScopeTarget({
          automatic: true,
          path: '.agents/skills',
          sharedPath: './.agents/skills',
          availability: 'shared-compatible',
          privatePath: '.firebender/skills',
        }),
      },
    };
    const details: SkillAgentDetails = {
      skillName: skill.name,
      scope: 'project',
      canonicalPath: skill.canonicalPath,
      automaticAgents: [],
      independentAgents: [],
      defaultAvailableAgents: [],
      privateRequiredAgents: [],
      duplicateCopyAgents: [{
        agent: 'firebender',
        displayName: 'Firebender',
        presence: 'duplicate-copy',
        sharedPath: '/canonical/agent-toolkit',
        privatePath: '/private/agent-toolkit',
        canCleanupPrivateCopy: true,
      }],
      privateOnlyAgents: [],
    };

    render(
      <ManageAgentsDialog
        skill={{ ...skill, agents: ['firebender'] }}
        scope="project"
        allAgents={[sharedCompatibleAgent]}
        agentDetails={details}
        onClose={vi.fn()}
        onSave={onSave}
      />
    );

    expect(screen.getByText('addSkill.agents.privateCopyTitle')).toBeDefined();
    expect(screen.getAllByText('Firebender').length).toBeGreaterThan(1);

    await user.click(screen.getByRole('button', { name: /Firebender/ }));
    await user.click(screen.getByRole('button', { name: 'skills.manageAgents.save' }));

    expect(onSave).toHaveBeenCalledWith([], [], 'symlink', [], ['firebender']);
  });

  it('includes duplicate copies in the dedicated copy section', () => {
    const sharedCompatibleAgent: AgentInfo = {
      id: 'firebender',
      name: 'Firebender',
      skillsDir: '.firebender/skills',
      globalSkillsDir: '~/.firebender/skills',
      detected: true,
      targets: {
        global: makeAgentScopeTarget({
          automatic: true,
          path: '~/.agents/skills',
          availability: 'shared-compatible',
          privatePath: '~/.firebender/skills',
        }),
        project: makeAgentScopeTarget({
          automatic: true,
          path: '.agents/skills',
          sharedPath: './.agents/skills',
          availability: 'shared-compatible',
          privatePath: '.firebender/skills',
        }),
      },
    };
    const details: SkillAgentDetails = {
      skillName: skill.name,
      scope: 'project',
      canonicalPath: skill.canonicalPath,
      automaticAgents: [['firebender', 'Firebender']],
      independentAgents: [],
      defaultAvailableAgents: [],
      privateRequiredAgents: [],
      duplicateCopyAgents: [{
        agent: 'firebender',
        displayName: 'Firebender',
        presence: 'duplicate-copy',
        sharedPath: '/canonical/agent-toolkit',
        privatePath: '/private/agent-toolkit',
        canCleanupPrivateCopy: true,
      }],
      privateOnlyAgents: [],
    };

    render(
      <ManageAgentsDialog
        skill={{ ...skill, agents: ['firebender'] }}
        scope="project"
        allAgents={[sharedCompatibleAgent]}
        agentDetails={details}
        onClose={vi.fn()}
        onSave={vi.fn()}
      />
    );

    expect(screen.getByText('addSkill.agents.privateCopyTitle')).toBeDefined();
    expect(screen.getAllByText('Firebender').length).toBeGreaterThan(1);
  });

  it('keeps content constrained inside the dialog when long paths are present', () => {
    const details: SkillAgentDetails = {
      skillName: skill.name,
      scope: 'project',
      canonicalPath: skill.canonicalPath,
      automaticAgents: [],
      independentAgents: [],
      defaultAvailableAgents: [],
      privateRequiredAgents: [],
      duplicateCopyAgents: [{
        agent: 'claude-code',
        displayName: 'Claude Code',
        presence: 'duplicate-copy',
        sharedPath: '/canonical/agent-toolkit',
        privatePath: '/Users/example/projects/very/long/path/that/should/not/push/dialog/width/.claude/skills',
        canCleanupPrivateCopy: true,
      }],
      privateOnlyAgents: [],
    };

    render(
      <ManageAgentsDialog
        skill={skill}
        scope="project"
        allAgents={allAgents}
        agentDetails={details}
        onClose={vi.fn()}
        onSave={vi.fn()}
      />
    );

    const dialog = screen.getByRole('dialog');
    expect(dialog.className).toContain('min-w-0');
    expect(dialog.className).toContain('max-w-[calc(100vw-2rem)]');

    const body = screen.getByTestId('manage-agents-dialog-body');
    expect(body.className).toContain('min-w-0');
    expect(body.className).toContain('max-w-full');

    expect(screen.queryByTestId('manage-agents-duplicate-row')).toBeNull();
  });
});
