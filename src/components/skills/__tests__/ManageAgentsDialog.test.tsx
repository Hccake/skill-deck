/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { ManageAgentsDialog } from '../ManageAgentsDialog';
import type { AgentInfo, InstalledSkill } from '@/bindings';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) =>
      values?.name ? `${key}:${values.name}` : key,
  }),
}));

const allAgents: AgentInfo[] = [
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
    id: 'cursor',
    name: 'Cursor',
    skillsDir: '.cursor/skills',
    globalSkillsDir: '~/.cursor/skills',
    detected: true,
    isUniversal: false,
    showInUniversalList: false,
  },
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
    const universalCursor: AgentInfo = {
      id: 'cursor',
      name: 'Cursor',
      skillsDir: '.agents/skills',
      globalSkillsDir: '~/.cursor/skills',
      detected: true,
      isUniversal: true,
      showInUniversalList: true,
    };
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
        allAgents={[universalCursor]}
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

    expect(onSave).toHaveBeenCalledWith(['cursor'], [], 'copy');
  });
});
