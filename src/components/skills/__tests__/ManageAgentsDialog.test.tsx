/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ManageAgentsDialog } from '../ManageAgentsDialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { InstalledSkill, ResolvedAgent, ManageAgentsPreview } from '@/bindings';
import { makeResolvedAgent } from '@/test-utils';
import { useMutationStore } from '@/stores/mutation';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) =>
      values?.name ? `${key}:${values.name}` : values?.path ? `${key}:${values.path}` : key,
  }),
}));

function makeAgent(agent: {
  id: string;
  name: string;
  detected: boolean;
  skillsDir: string;
  globalSkillsDir: string;
  globalAutomatic?: boolean;
  projectAutomatic?: boolean;
}): ResolvedAgent {
  return makeResolvedAgent({
    id: agent.id,
    displayName: agent.name,
    detection: agent.detected ? 'detected' : 'notDetected',
    global: {
      readsShared: agent.globalAutomatic ?? false,
      privatePath: agent.globalAutomatic ? null : agent.globalSkillsDir,
    },
    project: {
      readsShared: agent.projectAutomatic ?? false,
      sharedPath: './.agents/skills',
      privatePath: agent.projectAutomatic ? null : agent.skillsDir,
    },
  });
}

const allAgents: ResolvedAgent[] = [
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
  beforeEach(() => {
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
  });

  it('shows a lightweight loading shell before mounting the Agent selector', () => {
    render(
      <ManageAgentsDialog
        skill={skill}
        scope="project"
        allAgents={allAgents}
        loadingAgentDetails
        onClose={vi.fn()}
        onSave={vi.fn()}
      />
    );

    const dialog = screen.getByRole('dialog');
    const body = screen.getByTestId('manage-agents-dialog-body');
    expect(dialog.className).toContain('h-[min(38rem,calc(100dvh-2rem))]');
    expect(dialog.className).toContain('grid-rows-[auto_minmax(0,1fr)_auto]');
    expect(body.className).toContain('min-h-0');
    expect(body.className).toContain('overflow-y-auto');
    expect(body.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0);
    expect(screen.getByRole('status').textContent).toBe('common.loading');
    expect(screen.queryByText('Cursor')).toBeNull();
    expect(screen.getByRole('button', { name: 'common.cancel' })).not.toBeNull();
  });

  it('keeps the dialog open and offers retry when the preview fails', () => {
    const onRetry = vi.fn();
    render(
      <ManageAgentsDialog
        skill={skill}
        scope="project"
        allAgents={allAgents}
        previewFailed
        onRetry={onRetry}
        onClose={vi.fn()}
        onSave={vi.fn()}
      />
    );

    expect(screen.getByRole('alert').textContent).toBe('skills.manageAgents.previewError');
    fireEvent.click(screen.getByRole('button', { name: 'skills.manageAgents.retryPreview' }));
    expect(onRetry).toHaveBeenCalledOnce();
    expect(screen.getByRole('dialog')).not.toBeNull();
  });

  it('disables saving changes while another mutation is active', async () => {
    const user = userEvent.setup();
    useMutationStore.setState({
      activeMutation: {
        kind: 'update',
        context: { environment: { kind: 'host' }, scope: { scope: 'global' } },
        id: 'mutation-1',
        phase: 'preparing',
        progress: null,
        cancelable: true,
      },
    });

    render(
      <ManageAgentsDialog
        skill={skill}
        scope="project"
        allAgents={allAgents}
        onClose={vi.fn()}
        onSave={vi.fn()}
      />
    );

    await user.click(screen.getByText('Cursor'));
    expect((screen.getByRole('button', { name: 'skills.manageAgents.save' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('resets selected separate locations when agent metadata changes', () => {
    const automaticCursor: ResolvedAgent = makeAgent({
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

  it('checks existing separate Agent integrations without rendering physical entries', () => {
    const details = {
      token: {
        generation: 'existing-agent', registryRevision: 'registry-1',
        environmentRevision: 'environment-1', contextRevision: 'context-1',
      },
      context: { environment: { kind: 'host' }, scope: { scope: 'project', project_id: 'project-1' } },
      skillName: skill.name,
      availableAgents: allAgents,
      selectionGroups: { global: [], project: [] },
      observedEntries: [{
        entryId: 'physical-entry',
        displayPath: { environment: { kind: 'host' }, nativePath: '/private/agent-toolkit' },
        kind: 'directory',
        physicalTargetKey: 'host:/private/agent-toolkit',
        owners: [{ agentId: 'claude-code', displayName: 'Claude Code', logicalTargetId: 'claude-private' }],
        willBreakIfCanonicalRemoved: false,
      }],
      canonicalPayload: null,
      addTargets: [],
    } satisfies ManageAgentsPreview;

    render(
      <ManageAgentsDialog
        skill={{ ...skill, privateAdaptedAgents: ['claude-code'] }}
        scope="project"
        allAgents={allAgents}
        agentDetails={details}
        onClose={vi.fn()}
        onSave={vi.fn()}
      />
    );

    expect(screen.queryByText('skills.manageAgents.existingEntriesTitle')).toBeNull();
    expect(screen.queryByText('/private/agent-toolkit')).toBeNull();
    expect(screen.getByRole('checkbox', { name: /Claude Code/i }).getAttribute('aria-checked')).toBe('true');
  });

  it('submits Backend entry IDs when an existing separate integration is unchecked', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn().mockResolvedValue(undefined);
    const details = {
      token: {
        generation: 'remove-existing', registryRevision: 'registry-1',
        environmentRevision: 'environment-1', contextRevision: 'context-1',
      },
      context: { environment: { kind: 'host' }, scope: { scope: 'project', project_id: 'project-1' } },
      skillName: skill.name,
      availableAgents: allAgents,
      selectionGroups: { global: [], project: [] },
      observedEntries: [{
        entryId: 'claude-entry',
        displayPath: { environment: { kind: 'host' }, nativePath: '/private/agent-toolkit' },
        kind: 'directory',
        physicalTargetKey: 'host:/private/agent-toolkit',
        owners: [{ agentId: 'claude-code', displayName: 'Claude Code', logicalTargetId: 'claude-private' }],
        willBreakIfCanonicalRemoved: false,
      }],
      canonicalPayload: null,
      addTargets: [],
    } satisfies ManageAgentsPreview;

    render(
      <ManageAgentsDialog
        skill={{ ...skill, privateAdaptedAgents: ['claude-code'] }}
        scope="project"
        allAgents={allAgents}
        agentDetails={details}
        onClose={vi.fn()}
        onSave={onSave}
      />
    );

    await user.click(screen.getByRole('checkbox', { name: /Claude Code/i }));
    await user.click(screen.getByRole('button', { name: 'skills.manageAgents.save' }));

    expect(onSave).toHaveBeenCalledWith([], ['claude-entry'], 'symlink', []);
  });

  it('shows an undetected user-defined Agent from the current Registry preview', () => {
    const customAgent = makeResolvedAgent({
      id: 'my-custom-agent',
      displayName: 'My Custom Agent',
      source: 'custom',
      detection: 'notDetected',
      global: {
        readsShared: true,
        sharedPath: '~/.agents/skills',
        privatePath: '~/.my-custom-agent/skills',
      },
      project: {
        readsShared: true,
        sharedPath: './.agents/skills',
        privatePath: './.my-custom-agent/skills',
      },
    });

    render(
      <TooltipProvider>
        <ManageAgentsDialog
          skill={skill}
          scope="project"
          allAgents={[]}
          agentDetails={{
            token: {
              generation: 'manage-custom',
              registryRevision: 'registry-1',
              environmentRevision: 'environment-1',
              contextRevision: 'context-1',
            },
            context: { environment: { kind: 'host' }, scope: { scope: 'project', project_id: 'app' } },
            skillName: skill.name,
            availableAgents: [customAgent],
            selectionGroups: { global: [], project: [] },
            observedEntries: [],
            canonicalPayload: null,
            addTargets: [],
          }}
          onClose={vi.fn()}
          onSave={vi.fn()}
        />
      </TooltipProvider>
    );

    expect(screen.getByText('My Custom Agent')).toBeDefined();
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

    expect(onSave).toHaveBeenCalledWith(['cursor'], [], 'copy', []);
  });

  it('groups shared owners into one checkbox and removes the whole physical group', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn().mockResolvedValue(undefined);
    const details = {
      token: {
        generation: 'preview-1',
        registryRevision: 'registry-1',
        environmentRevision: 'environment-1',
        contextRevision: 'context-1',
      },
      context: {
        environment: { kind: 'host' },
        scope: { scope: 'project', project_id: 'project-1' },
      },
      skillName: skill.name,
      availableAgents: allAgents,
      selectionGroups: { global: [], project: [] },
      observedEntries: [{
        entryId: 'shared-physical-entry',
        displayPath: {
          environment: { kind: 'host' },
          nativePath: '/private/agent-toolkit',
        },
        kind: 'directory',
        physicalTargetKey: 'host:/private/agent-toolkit',
        owners: [
          { agentId: 'claude-code', displayName: 'Claude Code', logicalTargetId: 'claude-private' },
          { agentId: 'cursor', displayName: 'Cursor', logicalTargetId: 'cursor-private' },
        ],
        willBreakIfCanonicalRemoved: false,
      }],
      canonicalPayload: null,
      addTargets: [],
    } satisfies ManageAgentsPreview;

    render(
      <ManageAgentsDialog
        skill={skill}
        scope="project"
        allAgents={allAgents}
        agentDetails={details}
        onClose={vi.fn()}
        onSave={onSave}
      />
    );

    expect(screen.queryByText('/private/agent-toolkit')).toBeNull();
    expect(screen.queryByText('./.claude/skills')).toBeNull();
    const ownerGroup = screen.getByRole('checkbox', { name: /Claude Code.*Cursor/i });
    expect(ownerGroup.getAttribute('aria-checked')).toBe('true');
    expect(screen.queryByRole('checkbox', { name: /^Claude Code$/i })).toBeNull();
    expect(screen.queryByRole('checkbox', { name: /^Cursor$/i })).toBeNull();

    await user.click(ownerGroup);
    await user.click(screen.getByRole('button', { name: 'skills.manageAgents.save' }));

    expect(onSave).toHaveBeenCalledWith([], ['shared-physical-entry'], 'symlink', []);
  });

  it('removes a mixed required and optional owner group with one checkbox', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn().mockResolvedValue(undefined);
    const optionalAgent = makeResolvedAgent({
      id: 'firebender',
      displayName: 'Firebender',
      global: {
        readsShared: true,
        sharedPath: '~/.agents/skills',
        privatePath: '~/.firebender/skills',
      },
      project: {
        readsShared: true,
        sharedPath: './.agents/skills',
        privatePath: '.firebender/skills',
      },
    });
    const availableAgents = [allAgents[0], optionalAgent];
    const details = {
      token: {
        generation: 'mixed-owner-group', registryRevision: 'registry-1',
        environmentRevision: 'environment-1', contextRevision: 'context-1',
      },
      context: {
        environment: { kind: 'host' },
        scope: { scope: 'project', project_id: 'project-1' },
      },
      skillName: skill.name,
      availableAgents,
      selectionGroups: {
        global: [],
        project: [{
          groupId: 'mixed-owner-target',
          agentIds: ['claude-code', 'firebender'],
        }],
      },
      observedEntries: [{
        entryId: 'mixed-owner-entry',
        displayPath: { environment: { kind: 'host' }, nativePath: '/private/mixed-owner' },
        kind: 'directory',
        physicalTargetKey: 'host:/private/mixed-owner',
        owners: [
          { agentId: 'claude-code', displayName: 'Claude Code', logicalTargetId: 'claude-private' },
          { agentId: 'firebender', displayName: 'Firebender', logicalTargetId: 'firebender-private' },
        ],
        willBreakIfCanonicalRemoved: false,
      }],
      canonicalPayload: null,
      addTargets: [],
    } satisfies ManageAgentsPreview;

    render(
      <ManageAgentsDialog
        skill={skill}
        scope="project"
        allAgents={availableAgents}
        agentDetails={details}
        onClose={vi.fn()}
        onSave={onSave}
      />
    );

    const group = screen.getByRole('checkbox', { name: /Claude Code.*Firebender/i });
    expect(screen.getAllByRole('checkbox')).toHaveLength(1);
    await user.click(group);
    await user.click(screen.getByRole('button', { name: 'skills.manageAgents.save' }));

    expect(onSave).toHaveBeenCalledWith([], ['mixed-owner-entry'], 'symlink', []);
  });

  it('merges multiple physical entries owned by the same Agent into one removal choice', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn().mockResolvedValue(undefined);
    const details = {
      token: {
        generation: 'multi-entry', registryRevision: 'registry-1',
        environmentRevision: 'environment-1', contextRevision: 'context-1',
      },
      context: {
        environment: { kind: 'host' },
        scope: { scope: 'project', project_id: 'project-1' },
      },
      skillName: skill.name,
      availableAgents: allAgents,
      selectionGroups: { global: [], project: [] },
      observedEntries: ['entry-a', 'entry-b'].map((entryId) => ({
        entryId,
        displayPath: { environment: { kind: 'host' } as const, nativePath: `/private/${entryId}` },
        kind: 'directory' as const,
        physicalTargetKey: `host:/private/${entryId}`,
        owners: [{ agentId: 'claude-code', displayName: 'Claude Code', logicalTargetId: `${entryId}:claude` }],
        willBreakIfCanonicalRemoved: false,
      })),
      canonicalPayload: null,
      addTargets: [],
    } satisfies ManageAgentsPreview;

    render(
      <ManageAgentsDialog
        skill={skill}
        scope="project"
        allAgents={allAgents}
        agentDetails={details}
        onClose={vi.fn()}
        onSave={onSave}
      />
    );

    const checkbox = screen.getByRole('checkbox', { name: /Claude Code/i });
    expect(screen.getAllByRole('checkbox', { name: /Claude Code/i })).toHaveLength(1);

    await user.click(checkbox);
    await user.click(screen.getByRole('button', { name: 'skills.manageAgents.save' }));

    expect(onSave).toHaveBeenCalledWith([], ['entry-a', 'entry-b'], 'symlink', []);
  });

  it('keeps an existing optional Agent directory entry checked in Extra retain', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn().mockResolvedValue(undefined);
    const sharedCompatibleAgent = makeResolvedAgent({
      id: 'firebender',
      displayName: 'Firebender',
      global: {
          readsShared: true,
          sharedPath: '~/.agents/skills',
          privatePath: '~/.firebender/skills',
      },
      project: {
          readsShared: true,
          sharedPath: './.agents/skills',
          privatePath: '.firebender/skills',
      },
    });
    const details = {
      token: {
        generation: 'preview-firebender', registryRevision: 'registry-1',
        environmentRevision: 'environment-1', contextRevision: 'context-1',
      },
      context: {
        environment: { kind: 'host' },
        scope: { scope: 'project', project_id: 'project-1' },
      },
      skillName: skill.name,
      availableAgents: [sharedCompatibleAgent],
      selectionGroups: { global: [], project: [] },
      observedEntries: [{
        entryId: 'firebender-private-entry',
        displayPath: { environment: { kind: 'host' }, nativePath: '/private/agent-toolkit' },
        kind: 'directory',
        physicalTargetKey: 'host:/private/agent-toolkit',
        owners: [{
          agentId: 'firebender', displayName: 'Firebender', logicalTargetId: 'firebender-private',
        }],
        willBreakIfCanonicalRemoved: false,
      }],
      canonicalPayload: null,
      addTargets: [],
    } satisfies ManageAgentsPreview;

    render(
      <ManageAgentsDialog
        skill={{ ...skill, agents: ['firebender'], privateCopyAgents: ['firebender'] }}
        scope="project"
        allAgents={[sharedCompatibleAgent]}
        agentDetails={details}
        onClose={vi.fn()}
        onSave={onSave}
      />
    );

    expect(screen.getByText('addSkill.agents.privateCopyTitle')).toBeDefined();
    const checkbox = screen.getByRole('checkbox', { name: /Firebender/i });
    expect(checkbox.getAttribute('aria-checked')).toBe('true');
    expect(screen.queryByText('/private/agent-toolkit')).toBeNull();

    await user.click(checkbox);
    await user.click(screen.getByRole('button', { name: 'skills.manageAgents.save' }));

    expect(onSave).toHaveBeenCalledWith([], ['firebender-private-entry'], 'symlink', []);
  });

  it('adds a new optional Agent directory entry from Extra retain', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn().mockResolvedValue(undefined);
    const sharedCompatibleAgent = makeResolvedAgent({
      id: 'firebender',
      displayName: 'Firebender',
      global: {
          readsShared: true,
          sharedPath: '~/.agents/skills',
          privatePath: '~/.firebender/skills',
      },
      project: {
          readsShared: true,
          sharedPath: './.agents/skills',
          privatePath: '.firebender/skills',
      },
    });
    const details = {
      token: {
        generation: 'preview-owner-filter', registryRevision: 'registry-1',
        environmentRevision: 'environment-1', contextRevision: 'context-1',
      },
      context: {
        environment: { kind: 'host' },
        scope: { scope: 'project', project_id: 'project-1' },
      },
      skillName: skill.name,
      availableAgents: [sharedCompatibleAgent],
      selectionGroups: { global: [], project: [] },
      observedEntries: [],
      canonicalPayload: null,
      addTargets: [],
    } satisfies ManageAgentsPreview;

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

    await user.click(screen.getByText('addSkill.agents.privateCopyTitle'));
    await user.click(screen.getByRole('checkbox', { name: /Firebender/i }));
    await user.click(screen.getByText('addSkill.mode.copy'));
    await user.click(screen.getByRole('button', { name: 'skills.manageAgents.save' }));

    expect(onSave).toHaveBeenCalledWith([], [], 'copy', ['firebender']);
  });

  it('keeps content constrained inside the dialog when long paths are present', () => {
    const longPath = '/Users/example/projects/very/long/path/that/should/not/push/dialog/width/.claude/skills';
    const details = {
      token: {
        generation: 'preview-long-path', registryRevision: 'registry-1',
        environmentRevision: 'environment-1', contextRevision: 'context-1',
      },
      context: {
        environment: { kind: 'host' },
        scope: { scope: 'project', project_id: 'project-1' },
      },
      skillName: skill.name,
      availableAgents: allAgents,
      selectionGroups: { global: [], project: [] },
      observedEntries: [{
        entryId: 'long-path-entry',
        displayPath: { environment: { kind: 'host' }, nativePath: longPath },
        kind: 'directory',
        physicalTargetKey: `host:${longPath}`,
        owners: [{
          agentId: 'claude-code', displayName: 'Claude Code', logicalTargetId: 'claude-private',
        }],
        willBreakIfCanonicalRemoved: false,
      }],
      canonicalPayload: null,
      addTargets: [],
    } satisfies ManageAgentsPreview;

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

    expect(screen.queryByText(longPath)).toBeNull();
  });
});
