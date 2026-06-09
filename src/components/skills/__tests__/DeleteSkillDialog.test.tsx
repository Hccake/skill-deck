/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { DeleteSkillDialog } from '../DeleteSkillDialog';
import type { AgentType, InstalledSkill, SkillAgentDetails } from '@/bindings';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) =>
      values?.count ? `${key}:${values.count}` : key,
  }),
}));

const mockDeleteSkill = vi.fn();
const mockCloseDelete = vi.fn();

const skill: InstalledSkill = {
  name: 'agent-toolkit',
  description: 'Agent toolkit',
  path: '/skills/agent-toolkit',
  canonicalPath: '/canonical/agent-toolkit',
  scope: 'global',
  agents: ['firebender'],
};

const details: SkillAgentDetails = {
  skillName: skill.name,
  scope: 'global',
  canonicalPath: skill.canonicalPath,
  automaticAgents: [['firebender', 'Firebender']],
  independentAgents: [{
    agent: 'firebender',
    displayName: 'Firebender',
    path: '/private/agent-toolkit',
    isSymlink: false,
  }],
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

const mockDialogState = vi.hoisted(() => ({
  deleteTarget: null as { skill: InstalledSkill; scope: 'global'; projectPath?: string } | null,
  agentDetails: null as SkillAgentDetails | null,
  loadingAgentDetails: false,
}));

vi.mock('@/stores/skill-dialog', () => ({
  useSkillDialogStore: (selector: (state: unknown) => unknown) => selector({
    ...mockDialogState,
    closeDelete: mockCloseDelete,
    deleteSkill: mockDeleteSkill,
  }),
}));

describe('DeleteSkillDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockDialogState.deleteTarget = { skill, scope: 'global' };
    mockDialogState.agentDetails = details;
    mockDialogState.loadingAgentDetails = false;
  });

  it('does not force dedicated copies to be deleted when deleting from the shared Skill directory', async () => {
    const user = userEvent.setup();
    mockDeleteSkill.mockResolvedValue(undefined);

    render(<DeleteSkillDialog />);

    const independentCheckbox = screen.getByLabelText('Firebender') as HTMLButtonElement;
    expect(independentCheckbox.getAttribute('data-state')).toBe('unchecked');

    await user.click(screen.getByLabelText('skills.deleteConfirm.deleteCanonical'));

    expect(independentCheckbox.disabled).toBe(false);
    expect(independentCheckbox.getAttribute('data-state')).toBe('unchecked');
    expect(screen.getByText('skills.deleteConfirm.canonicalLeavesPrivateCopiesWarning')).toBeDefined();

    await user.click(screen.getByRole('button', { name: 'skills.deleteConfirm.confirm' }));

    expect(mockDeleteSkill).toHaveBeenCalledWith({
      fullRemoval: true,
      agents: [] as AgentType[],
    });
  });
});
