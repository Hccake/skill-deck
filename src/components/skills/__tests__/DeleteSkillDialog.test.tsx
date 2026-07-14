/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { DeleteSkillDialog } from '../DeleteSkillDialog';
import type { AgentType, InstalledSkill, SkillAgentDetails, SkillScope } from '@/bindings';
import { useMutationStore } from '@/stores/mutation';

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
  eveTargets: [],
};

const mockDialogState = vi.hoisted(() => ({
  deleteTarget: null as { skill: InstalledSkill; scope: SkillScope; projectPath?: string } | null,
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
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
  });

  it('disables deletion while another mutation is active', () => {
    useMutationStore.setState({
      activeMutation: {
        kind: 'install',
        context: { environment: { kind: 'host' }, scope: { scope: 'global' } },
        statusText: 'Installing',
        cancelable: true,
      },
    });

    render(<DeleteSkillDialog />);

    fireEvent.click(screen.getByLabelText('skills.deleteConfirm.deleteCanonical'));
    expect((screen.getByRole('button', { name: 'skills.deleteConfirm.confirm' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('does not force Agent directory entries to be deleted when deleting from the shared Skill directory', async () => {
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

  it('shows Eve targets and submits concrete target specs for partial deletion', async () => {
    const user = userEvent.setup();
    mockDeleteSkill.mockResolvedValue(undefined);
    mockDialogState.deleteTarget = {
      skill: { ...skill, scope: 'project', agents: ['eve'] },
      scope: 'project',
      projectPath: '/projects/eve-app',
    };
    mockDialogState.agentDetails = {
      ...details,
      scope: 'project',
      automaticAgents: [],
      independentAgents: [],
      eveTargets: [
        {
          targetId: 'eve:root',
          agent: 'eve',
          displayName: 'Eve (root)',
          subagent: null,
          path: '/projects/eve-app/agent/skills/agent-toolkit',
        },
        {
          targetId: 'eve:research',
          agent: 'eve',
          displayName: 'Eve (research)',
          subagent: 'research',
          path: '/projects/eve-app/agent/subagents/research/skills/agent-toolkit',
        },
      ],
    };

    render(<DeleteSkillDialog />);

    expect(screen.getByLabelText('Eve (root)')).toBeDefined();
    expect(screen.getByLabelText('Eve (research)')).toBeDefined();

    await user.click(screen.getByLabelText('Eve (root)'));
    await user.click(screen.getByRole('button', { name: 'skills.deleteConfirm.confirmPartial:1' }));

    expect(mockDeleteSkill).toHaveBeenCalledWith({
      fullRemoval: false,
      agents: [],
      agentTargets: [{ agent: 'eve', subagent: 'research' }],
    });
  });

  it('submits the selected Eve targets when deleting the shared Skill directory', async () => {
    const user = userEvent.setup();
    mockDeleteSkill.mockResolvedValue(undefined);
    mockDialogState.deleteTarget = {
      skill: { ...skill, scope: 'project', agents: ['eve', 'firebender'] },
      scope: 'project',
      projectPath: '/projects/eve-app',
    };
    mockDialogState.agentDetails = {
      ...details,
      scope: 'project',
      eveTargets: [
        {
          targetId: 'eve:root',
          agent: 'eve',
          displayName: 'Eve (root)',
          subagent: null,
          path: '/projects/eve-app/agent/skills/agent-toolkit',
        },
        {
          targetId: 'eve:research',
          agent: 'eve',
          displayName: 'Eve (research)',
          subagent: 'research',
          path: '/projects/eve-app/agent/subagents/research/skills/agent-toolkit',
        },
      ],
    };

    render(<DeleteSkillDialog />);

    await user.click(screen.getByLabelText('skills.deleteConfirm.deleteCanonical'));
    await user.click(screen.getByLabelText('Eve (root)'));
    await user.click(screen.getByRole('button', { name: 'skills.deleteConfirm.confirm' }));

    expect(mockDeleteSkill).toHaveBeenCalledWith({
      fullRemoval: true,
      agents: [] as AgentType[],
      agentTargets: [{ agent: 'eve', subagent: 'research' }],
    });
  });

  it('submits an explicit empty Eve target list when all Eve targets are kept', async () => {
    const user = userEvent.setup();
    mockDeleteSkill.mockResolvedValue(undefined);
    mockDialogState.deleteTarget = {
      skill: { ...skill, scope: 'project', agents: ['eve', 'firebender'] },
      scope: 'project',
      projectPath: '/projects/eve-app',
    };
    mockDialogState.agentDetails = {
      ...details,
      scope: 'project',
      eveTargets: [
        {
          targetId: 'eve:root',
          agent: 'eve',
          displayName: 'Eve (root)',
          subagent: null,
          path: '/projects/eve-app/agent/skills/agent-toolkit',
        },
        {
          targetId: 'eve:research',
          agent: 'eve',
          displayName: 'Eve (research)',
          subagent: 'research',
          path: '/projects/eve-app/agent/subagents/research/skills/agent-toolkit',
        },
      ],
    };

    render(<DeleteSkillDialog />);

    await user.click(screen.getByLabelText('skills.deleteConfirm.deleteCanonical'));
    await user.click(screen.getByLabelText('Eve (root)'));
    await user.click(screen.getByLabelText('Eve (research)'));
    await user.click(screen.getByRole('button', { name: 'skills.deleteConfirm.confirm' }));

    expect(mockDeleteSkill).toHaveBeenCalledWith({
      fullRemoval: true,
      agents: [] as AgentType[],
      agentTargets: [],
    });
  });
});
