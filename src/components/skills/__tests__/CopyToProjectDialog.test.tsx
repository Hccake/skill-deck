/* @vitest-environment jsdom */

import '@/test-utils';
import type { ReactElement } from 'react';
import { fireEvent, render as testingLibraryRender, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { CopyToProjectDialog } from '../CopyToProjectDialog';
import type { InstalledSkill, MutationUnitResult } from '@/bindings';
import { useMutationStore } from '@/stores/mutation';
import type { CopyOutcome } from '@/workflows/skill-copy';
import { makeAgentSelectionSnapshot } from '@/test-utils';
import { TooltipProvider } from '@/components/ui/tooltip';

function render(ui: ReactElement) {
  return testingLibraryRender(ui, { wrapper: TooltipProvider });
}

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      if (key === 'skills.copyToProject.description') return `description:${options?.name}`;
      return key;
    },
  }),
}));

const skill = (overrides: Partial<InstalledSkill> = {}): InstalledSkill => ({
  name: 'toolkit',
  description: '',
  path: '/project/.agents/skills/toolkit',
  canonicalPath: '/project/.agents/skills/toolkit',
  scope: 'project',
  agents: ['claude-code'],
  associatedAgents: ['claude-code'],
  source: 'owner/repo',
  sourceUrl: 'https://github.com/owner/repo',
  canRunUpdate: true,
  canCheckForUpdates: true,
  updateReason: null,
  ...overrides,
});

const defaultCopyProps = {
  sourceContext: {
    environment: { kind: 'native' as const },
    scope: { scope: 'project' as const, project_id: 'project-a' },
  },
  environments: [
    {
      environment: { kind: 'native' as const },
      displayName: 'Native',
      status: 'available' as const,
      revision: 1,
      error: null,
    },
  ],
  projectsByEnvironment: {
    native: ['/project-a', '/project-b'].map((nativePath) => ({
      binding: {
        id: nativePath.slice(1),
        nativePath,
        displayName: null,
        order: null,
        suppressCrossStorageWarning: false,
      },
      storage: { access: 'native' as const, owner: null },
    })),
  },
  onLoadProjects: vi.fn(async () => undefined),
  agentSelection: {
    status: 'ready' as const,
    snapshot: {
      selection: makeAgentSelectionSnapshot({ revision: 'copy-selection-1' }),
    },
    retry: vi.fn(async () => undefined),
  },
};

const selectableAgentState = {
  status: 'ready' as const,
  snapshot: {
    selection: makeAgentSelectionSnapshot({
      revision: 'copy-selection-2',
      agents: [{
        kind: 'standard' as const,
        id: 'claude-code',
        displayName: 'Claude Code',
        detection: 'detected' as const,
        directoryAccess: 'privateOnly' as const,
        installOptionId: 'claude',
        groupId: null,
      }],
      installOptions: [{
        id: 'claude',
        kind: 'standardDirectory' as const,
        agentIds: ['claude-code'],
        displayName: 'Claude Code',
        path: '~/.claude/skills',
        groupId: null,
        selectable: true,
        modeConstraint: 'userSelectable' as const,
        disabledReason: null,
      }],
      userModeOptionIds: ['claude'],
    }),
  },
  retry: vi.fn(async () => undefined),
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function failedCopyUnit(projectId: string, retryable = true): MutationUnitResult {
  return {
    unitId: `copy:toolkit:${projectId}`,
    skillName: 'toolkit',
    source: defaultCopyProps.sourceContext,
    target: {
      environment: { kind: 'native' },
      scope: { scope: 'project', project_id: projectId },
    },
    status: 'failed',
    retryable,
    lockCommitted: false,
    actualMode: null,
    fallbackReason: null,
    agentTargets: [],
    warnings: [],
    error: {
      code: 'configurationCorrupted',
      parameters: {},
      field: null,
      severity: 'error',
      retryable,
      technicalDetails: 'private diagnostic',
      environment: { kind: 'native' },
      context: null,
      unitId: `copy:toolkit:${projectId}`,
      recoveryResourceId: null,
      displayPaths: [],
    },
    recovery: null,
  };
}

describe('CopyToProjectDialog', () => {
  beforeEach(() => {
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
    Element.prototype.scrollIntoView = vi.fn();
  });

  it('keeps a stable dialog frame with independently scrolling project and Agent areas', () => {
    const projectLoad = deferred<void>();
    render(
      <CopyToProjectDialog
        skill={skill()}
        {...defaultCopyProps}
        onLoadProjects={() => projectLoad.promise}
        onClose={vi.fn()}
        onCopy={vi.fn()}
      />
    );

    const dialog = screen.getByRole('dialog');
    const body = screen.getByTestId('copy-to-project-dialog-body');
    const projectScrollArea = screen.getByTestId('copy-target-projects-scroll');
    const agentScrollArea = screen.getByTestId('copy-agent-settings-scroll');
    expect(dialog.className).toContain('h-[min(42rem,calc(100dvh-2rem))]');
    expect(dialog.className).toContain('grid-rows-[auto_minmax(0,1fr)_auto]');
    expect(body.className).toContain('min-h-0');
    expect(body.className).toContain('overflow-hidden');
    expect(body.className).not.toContain('overflow-y-auto');
    expect(projectScrollArea).not.toBe(agentScrollArea);
    expect(projectScrollArea.className).toContain('min-h-0');
    expect(projectScrollArea.className).toContain('overflow-y-auto');
    expect(agentScrollArea.className).toContain('min-h-0');
    expect(agentScrollArea.className).toContain('overflow-y-auto');
    expect(body.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0);
    expect(screen.getByRole('button', { name: 'common.cancel' })).not.toBeNull();
  });

  it('disables copying while another mutation is active', async () => {
    useMutationStore.setState({
      activeMutation: {
        kind: 'update',
        context: { environment: { kind: 'native' }, scope: { scope: 'global' } },
        id: 'mutation-1',
        phase: 'preparing',
        progress: null,
        cancelable: true,
      },
    });
    render(
      <CopyToProjectDialog
        skill={skill()}
        {...defaultCopyProps}
        onClose={vi.fn()}
        onCopy={vi.fn()}
      />
    );

    fireEvent.click(await screen.findByText('/project-b'));
    expect((screen.getByRole('button', { name: 'skills.copyToProject.copy' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('keeps project and Agent configuration independent until copy is confirmed', async () => {
    const onCopy = vi.fn(async () => ({
      status: 'succeeded' as const,
      response: { units: [] },
      succeededProjectIds: ['project-b'],
    }));
    render(
      <CopyToProjectDialog
        skill={skill()}
        {...defaultCopyProps}
        agentSelection={selectableAgentState}
        onClose={vi.fn()}
        onCopy={onCopy}
      />
    );

    expect(screen.queryByRole('combobox', {
      name: 'skills.copyToProject.targetEnvironment',
    })).toBeNull();
    fireEvent.click(await screen.findByRole('checkbox', { name: 'Claude Code' }));
    fireEvent.click(screen.getByText('agentSelection.copy'));
    fireEvent.click(await screen.findByText('/project-b'));
    fireEvent.click(screen.getByRole('button', { name: 'skills.copyToProject.copy' }));

    await waitFor(() => expect(onCopy).toHaveBeenCalledWith({
      environment: { kind: 'native' },
      projectIds: ['project-b'],
      agentSelection: {
        revision: 'copy-selection-2',
        selectedOptionIds: ['claude'],
        requestedMode: 'copy',
      },
    }));
  });

  it('keeps target projects selected and requires confirmation when Agent status changes', async () => {
    const refreshedSelection = makeAgentSelectionSnapshot({
      revision: 'copy-selection-3',
      agents: selectableAgentState.snapshot.selection.agents,
      installOptions: selectableAgentState.snapshot.selection.installOptions,
      initialSelectedOptionIds: ['claude'],
      userModeOptionIds: ['claude'],
    });
    const onCopy = vi.fn()
      .mockResolvedValueOnce({
        status: 'selectionStale' as const,
        snapshot: { selection: refreshedSelection },
      })
      .mockResolvedValueOnce({
        status: 'succeeded' as const,
        response: { units: [] },
        succeededProjectIds: ['project-b'],
      });
    render(
      <CopyToProjectDialog
        skill={skill()}
        {...defaultCopyProps}
        agentSelection={selectableAgentState}
        onClose={vi.fn()}
        onCopy={onCopy}
      />
    );

    fireEvent.click(await screen.findByText('/project-b'));
    fireEvent.click(screen.getByRole('button', { name: 'skills.copyToProject.copy' }));

    const confirm = await screen.findByRole('button', {
      name: 'agentSelection.confirmCurrentSelection',
    });
    expect((screen.getByRole('button', {
      name: 'skills.copyToProject.copy',
    }) as HTMLButtonElement).disabled).toBe(true);
    const selectedProjectCheckbox = screen.getByText('/project-b')
      .closest('label')
      ?.querySelector<HTMLButtonElement>('[role="checkbox"]');
    expect(selectedProjectCheckbox?.dataset.state).toBe('checked');

    fireEvent.click(confirm);
    const copy = screen.getByRole('button', { name: 'skills.copyToProject.copy' });
    expect((copy as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(copy);

    await waitFor(() => expect(onCopy).toHaveBeenLastCalledWith({
      environment: { kind: 'native' },
      projectIds: ['project-b'],
      agentSelection: {
        revision: 'copy-selection-3',
        selectedOptionIds: [],
        requestedMode: 'symlink',
      },
    }));
  });

  it('selects exactly one target environment and submits project IDs from that environment', async () => {
    const onCopy = vi.fn(async () => ({
      status: 'succeeded',
      response: { units: [] },
      succeededProjectIds: ['native-target'],
    } satisfies CopyOutcome));
    render(
      <CopyToProjectDialog
        skill={skill()}
        sourceContext={{
          environment: { kind: 'wsl', distro_name: 'Ubuntu' },
          scope: { scope: 'project', project_id: 'source' },
        }}
        environments={[
          { environment: { kind: 'native' }, displayName: 'Windows', status: 'available', revision: 1, error: null },
          { environment: { kind: 'wsl', distro_name: 'Ubuntu' }, displayName: 'Ubuntu', status: 'available', revision: 1, error: null },
        ]}
        projectsByEnvironment={{
          native: [{
            binding: {
              id: 'native-target', nativePath: 'C:\\Code\\target', displayName: 'Native target',
              order: 0, suppressCrossStorageWarning: false,
            },
            storage: { access: 'native', owner: null },
          }],
          'wsl:ubuntu': [{
            binding: {
              id: 'source', nativePath: '/home/me/source', displayName: 'Source',
              order: 0, suppressCrossStorageWarning: false,
            },
            storage: { access: 'native', owner: null },
          }],
        }}
        agentSelection={defaultCopyProps.agentSelection}
        onLoadProjects={vi.fn(async () => undefined)}
        onClose={vi.fn()}
        onCopy={onCopy}
      />
    );

    fireEvent.click(screen.getByRole('combobox', { name: 'skills.copyToProject.targetEnvironment' }));
    fireEvent.click(await screen.findByRole('option', { name: 'Windows' }));
    fireEvent.click(await screen.findByText('Native target'));
    fireEvent.click(screen.getByRole('button', { name: 'skills.copyToProject.copy' }));

    await waitFor(() => {
      expect(onCopy).toHaveBeenCalledWith({
        environment: { kind: 'native' },
        projectIds: ['native-target'],
        agentSelection: {
          revision: 'copy-selection-1',
          selectedOptionIds: [],
          requestedMode: 'symlink',
        },
      });
      expect((screen.getByRole('button', {
        name: 'skills.copyToProject.copy',
      }) as HTMLButtonElement).disabled).toBe(false);
    });
  });

  it('requires an explicit reselect after the target Environment disappears', async () => {
    const ubuntuEnvironment = {
      environment: { kind: 'wsl' as const, distro_name: 'Ubuntu' },
      displayName: 'Ubuntu',
      status: 'available' as const,
      revision: 1,
      error: null,
    };
    const ubuntuProject = {
      binding: {
        id: 'ubuntu-target',
        nativePath: '/home/me/target',
        displayName: null,
        order: null,
        suppressCrossStorageWarning: false,
      },
      storage: {
        access: 'native' as const,
        owner: ubuntuEnvironment.environment,
      },
    };
    const props = {
      skill: skill(),
      ...defaultCopyProps,
      environments: [...defaultCopyProps.environments, ubuntuEnvironment],
      projectsByEnvironment: {
        ...defaultCopyProps.projectsByEnvironment,
        'wsl:ubuntu': [ubuntuProject],
      },
      agentSelection: selectableAgentState,
      onLoadProjects: vi.fn(async () => undefined),
      onClose: vi.fn(),
      onCopy: vi.fn(),
    };
    const view = render(<CopyToProjectDialog {...props} />);

    fireEvent.click(screen.getByRole('combobox', {
      name: 'skills.copyToProject.targetEnvironment',
    }));
    fireEvent.click(await screen.findByRole('option', { name: 'Ubuntu' }));
    fireEvent.click(await screen.findByText('/home/me/target'));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Claude Code' }));

    view.rerender(
      <CopyToProjectDialog {...props} environments={defaultCopyProps.environments} />,
    );

    expect((await screen.findAllByText('skills.copyToProject.targetEnvironmentMissing')).length)
      .toBeGreaterThan(0);
    expect(screen.queryByText('/home/me/target')).toBeNull();
    expect((screen.getByRole('checkbox', { name: 'Claude Code' }) as HTMLButtonElement).dataset.state)
      .toBe('checked');
    expect((screen.getByRole('button', {
      name: 'skills.copyToProject.copy',
    }) as HTMLButtonElement).disabled).toBe(true);

    view.rerender(
      <CopyToProjectDialog {...props} />,
    );
    expect(screen.getAllByText('skills.copyToProject.targetEnvironmentMissing').length)
      .toBeGreaterThan(0);
    expect(screen.queryByText('/home/me/target')).toBeNull();

    fireEvent.click(screen.getByRole('combobox', {
      name: 'skills.copyToProject.targetEnvironment',
    }));
    fireEvent.click(await screen.findByRole('option', { name: 'Ubuntu' }));
    expect(await screen.findByText('/home/me/target')).toBeDefined();
    expect((screen.getByRole('checkbox', { name: 'Claude Code' }) as HTMLButtonElement).dataset.state)
      .toBe('checked');
  });

  it('keeps partial copy results in the dialog and excludes completed projects from retry', async () => {
    const onCopy = vi.fn(async () => ({
      status: 'partial' as const,
      response: { units: [failedCopyUnit('project-c')] },
      succeededProjectIds: ['project-b'],
      failedProjectIds: ['project-c'],
      retryableProjectIds: ['project-c'],
    } satisfies CopyOutcome));
    const projects = [
      ...defaultCopyProps.projectsByEnvironment.native,
      {
        binding: {
          id: 'project-c', nativePath: '/project-c', displayName: null,
          order: null, suppressCrossStorageWarning: false,
        },
        storage: { access: 'native' as const, owner: null },
      },
    ];
    render(
      <CopyToProjectDialog
        skill={skill()}
        {...defaultCopyProps}
        projectsByEnvironment={{ native: projects }}
        onClose={vi.fn()}
        onCopy={onCopy}
      />
    );

    const checkboxes = await screen.findAllByRole('checkbox');
    fireEvent.click(checkboxes[0]);
    fireEvent.click(checkboxes[1]);
    fireEvent.click(screen.getByRole('button', { name: 'skills.copyToProject.copy' }));

    expect((await screen.findByRole('alert')).textContent)
      .toContain('skills.copyToProject.partialError');
    expect(screen.queryByText('/project-b')).toBeNull();
    expect(screen.getByText('mutation.result.errors.configurationCorrupted')).toBeDefined();
    expect(screen.queryByText('private diagnostic')).toBeNull();
    expect((screen.getByRole('checkbox') as HTMLButtonElement).dataset.state).toBe('checked');
    expect(screen.getByRole('button', { name: 'skills.copyToProject.retryFailed' })).toBeDefined();
  });

  it('starts a fresh selection session when the source skill changes', async () => {
    const { rerender } = render(
      <CopyToProjectDialog
        skill={skill()}
        {...defaultCopyProps}
        onClose={vi.fn()}
        onCopy={vi.fn()}
      />
    );

    const checkbox = await screen.findByRole('checkbox');
    fireEvent.click(checkbox);
    expect((checkbox as HTMLButtonElement).dataset.state).toBe('checked');

    rerender(
      <CopyToProjectDialog
        skill={skill({
          name: 'other-skill',
          path: '/project/.agents/skills/other-skill',
          canonicalPath: '/project/.agents/skills/other-skill',
        })}
        {...defaultCopyProps}
        onClose={vi.fn()}
        onCopy={vi.fn()}
      />
    );

    const resetCheckbox = await screen.findByRole('checkbox');
    expect((resetCheckbox as HTMLButtonElement).dataset.state).toBe('unchecked');
    expect((screen.getByRole('button', {
      name: 'skills.copyToProject.copy',
    }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('does not show a source note when copied skill can keep update metadata', async () => {
    render(
      <CopyToProjectDialog
        skill={skill()}
        {...defaultCopyProps}
        onClose={vi.fn()}
        onCopy={vi.fn()}
      />
    );

    await screen.findByText('/project-b');
    expect(screen.queryByText('skills.copyToProject.metadataWarning')).toBeNull();
  });

  it('uses the shared project name fallback and keeps the full path visible', async () => {
    render(
      <CopyToProjectDialog
        skill={skill()}
        {...defaultCopyProps}
        onClose={vi.fn()}
        onCopy={vi.fn()}
      />
    );

    expect(await screen.findByText('project-b')).toBeDefined();
    expect(screen.getByText('/project-b')).toBeDefined();
  });

  it('does not show update metadata warnings in the copy flow', async () => {
    render(
      <CopyToProjectDialog
        skill={skill({
          canRunUpdate: true,
          canCheckForUpdates: false,
          updateReason: 'missingRemoteHash',
        })}
        {...defaultCopyProps}
        onClose={vi.fn()}
        onCopy={vi.fn()}
      />
    );

    await screen.findByText('/project-b');
    expect(screen.queryByRole('note')).toBeNull();
  });

  it('does not add a copy-specific warning for a local source', async () => {
    render(
      <CopyToProjectDialog
        skill={skill({
          source: '/home/alice/skills',
          sourceUrl: null,
          canRunUpdate: false,
          canCheckForUpdates: false,
          updateReason: 'local-source',
        })}
        {...defaultCopyProps}
        onClose={vi.fn()}
        onCopy={vi.fn()}
      />
    );

    await screen.findByText('/project-b');
    expect(screen.queryByRole('note')).toBeNull();
  });

  it('does not show the source note for temporary update check failures', async () => {
    render(
      <CopyToProjectDialog
        skill={skill({
          canRunUpdate: true,
          canCheckForUpdates: false,
          updateReason: 'network-error',
        })}
        {...defaultCopyProps}
        onClose={vi.fn()}
        onCopy={vi.fn()}
      />
    );

    await screen.findByText('/project-b');
    expect(screen.queryByText('skills.copyToProject.metadataWarning')).toBeNull();
  });

  it('keeps ordinary copy failures as retryable copy feedback', async () => {
    const onCopy = vi.fn(async () => ({
      status: 'failed' as const,
      error: { kind: 'staleContext' as const },
    } satisfies CopyOutcome));
    render(
      <CopyToProjectDialog
        skill={skill()}
        {...defaultCopyProps}
        onClose={vi.fn()}
        onCopy={onCopy}
      />
    );

    fireEvent.click(await screen.findByRole('checkbox'));
    fireEvent.click(screen.getByRole('button', { name: 'skills.copyToProject.copy' }));

    expect((await screen.findByRole('alert')).textContent)
      .toContain('skills.copyToProject.copyError');
  });

  it('shows the structured reason and clears automatic retry for a non-retryable project failure', async () => {
    const onCopy = vi.fn(async () => ({
      status: 'failed' as const,
      unit: failedCopyUnit('project-b', false),
    } satisfies CopyOutcome));
    render(
      <CopyToProjectDialog
        skill={skill()}
        {...defaultCopyProps}
        onClose={vi.fn()}
        onCopy={onCopy}
      />
    );

    fireEvent.click(await screen.findByRole('checkbox'));
    fireEvent.click(screen.getByRole('button', { name: 'skills.copyToProject.copy' }));

    expect((await screen.findByRole('alert')).textContent)
      .toContain('mutation.result.errors.configurationCorrupted');
    expect((screen.getByRole('checkbox') as HTMLButtonElement).dataset.state).toBe('unchecked');
    expect(screen.queryByRole('button', { name: 'skills.copyToProject.retryFailed' })).toBeNull();
  });

  it('shows a recoverable error when target projects cannot be loaded', async () => {
    const onLoadProjects = vi.fn()
      .mockRejectedValueOnce(new Error('WSL unavailable'))
      .mockResolvedValueOnce(undefined);

    render(
      <CopyToProjectDialog
        skill={skill()}
        {...defaultCopyProps}
        onLoadProjects={onLoadProjects}
        onClose={vi.fn()}
        onCopy={vi.fn()}
      />
    );

    expect((await screen.findByRole('alert')).textContent).toContain(
      'skills.copyToProject.projectsLoadError',
    );
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }));
    await waitFor(() => expect(onLoadProjects).toHaveBeenCalledTimes(2));
  });

  it('distinguishes target Environment connection failure from project loading failure', async () => {
    render(
      <CopyToProjectDialog
        skill={skill()}
        {...defaultCopyProps}
        onLoadProjects={vi.fn().mockRejectedValue({
          status: 'failed',
          failureSource: 'environment',
          error: {
            kind: 'environmentUnavailable',
            data: { environment: { kind: 'native' }, message: 'unavailable' },
          },
        })}
        onClose={vi.fn()}
        onCopy={vi.fn()}
      />
    );

    expect((await screen.findByRole('alert')).textContent).toContain(
      'skills.copyToProject.targetEnvironmentConnectionError',
    );
    expect(screen.queryByText('skills.copyToProject.projectsLoadError')).toBeNull();
  });

  it('shows unknown presence instead of treating a failed check as absent', async () => {
    render(
      <CopyToProjectDialog
        skill={skill()}
        {...defaultCopyProps}
        checkExistence={vi.fn().mockRejectedValue(new Error('inspection failed'))}
        onClose={vi.fn()}
        onCopy={vi.fn()}
      />
    );

    expect(await screen.findByRole('status', {
      name: 'skills.copyToProject.presenceUnknown',
    })).toBeDefined();
    expect(screen.getByText('/project-b').closest('label')?.textContent).toContain(
      'skills.copyToProject.unknown',
    );
  });

  it('ignores stale presence results after switching environments', async () => {
    const nativePresence = deferred<Array<{ projectId: string; hasSkill: boolean }>>();
    const ubuntuPresence = deferred<Array<{ projectId: string; hasSkill: boolean }>>();
    const checkExistence = vi.fn((
      _skillName: string,
      environment: { kind: string },
    ) => environment.kind === 'native' ? nativePresence.promise : ubuntuPresence.promise);

    render(
      <CopyToProjectDialog
        skill={skill()}
        sourceContext={defaultCopyProps.sourceContext}
        environments={[
          ...defaultCopyProps.environments,
          {
            environment: { kind: 'wsl' as const, distro_name: 'Ubuntu' },
            displayName: 'Ubuntu',
            status: 'available' as const,
            revision: 1,
            error: null,
          },
        ]}
        projectsByEnvironment={{
          ...defaultCopyProps.projectsByEnvironment,
          'wsl:ubuntu': [{
            binding: {
              id: 'ubuntu-target',
              nativePath: '/home/me/target',
              displayName: null,
              order: null,
              suppressCrossStorageWarning: false,
            },
            storage: { access: 'native', owner: { kind: 'wsl' as const, distro_name: 'Ubuntu' } },
          }],
        }}
        agentSelection={defaultCopyProps.agentSelection}
        onLoadProjects={vi.fn(async () => undefined)}
        checkExistence={checkExistence}
        onClose={vi.fn()}
        onCopy={vi.fn()}
      />
    );

    fireEvent.click(screen.getByRole('combobox', { name: 'skills.copyToProject.targetEnvironment' }));
    fireEvent.click(await screen.findByRole('option', { name: 'Ubuntu' }));
    ubuntuPresence.resolve([{ projectId: 'ubuntu-target', hasSkill: false }]);
    await waitFor(() => expect(screen.getByText('/home/me/target')).toBeDefined());
    nativePresence.resolve([{ projectId: 'project-b', hasSkill: true }]);

    await waitFor(() => {
      expect(screen.queryByText('skills.copyToProject.installed')).toBeNull();
    });
  });
});
