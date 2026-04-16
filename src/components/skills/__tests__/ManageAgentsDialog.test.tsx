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
