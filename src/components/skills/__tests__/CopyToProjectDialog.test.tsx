/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { CopyToProjectDialog } from '../CopyToProjectDialog';
import type { InstalledSkill } from '@/bindings';
import { useMutationStore } from '@/stores/mutation';
import type { CopyOutcome } from '@/workflows/skill-copy';

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
  source: 'owner/repo',
  sourceUrl: 'https://github.com/owner/repo',
  canRunUpdate: true,
  canCheckForUpdates: true,
  updateReason: null,
  ...overrides,
});

const defaultCopyProps = {
  sourceContext: {
    environment: { kind: 'host' as const },
    scope: { scope: 'project' as const, project_id: 'project-a' },
  },
  environments: [
    {
      environment: { kind: 'host' as const },
      displayName: 'Host',
      status: 'available' as const,
      revision: 1,
      error: null,
    },
  ],
  projectsByEnvironment: {
    host: ['/project-a', '/project-b'].map((nativePath) => ({
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

describe('CopyToProjectDialog', () => {
  beforeEach(() => {
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
    Element.prototype.scrollIntoView = vi.fn();
  });

  it('keeps a stable dialog frame while target projects are loading', () => {
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
    expect(dialog.className).toContain('h-[min(32rem,calc(100dvh-2rem))]');
    expect(dialog.className).toContain('grid-rows-[auto_minmax(0,1fr)_auto]');
    expect(body.className).toContain('min-h-0');
    expect(body.className).toContain('overflow-y-auto');
    expect(body.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0);
    expect(screen.getByRole('button', { name: 'common.cancel' })).not.toBeNull();
  });

  it('disables copying while another mutation is active', async () => {
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

  it('selects exactly one target environment and submits project IDs from that environment', async () => {
    const onCopy = vi.fn(async () => ({
      status: 'succeeded',
      response: { units: [] },
      succeededProjectIds: ['host-target'],
    } satisfies CopyOutcome));
    render(
      <CopyToProjectDialog
        skill={skill()}
        sourceContext={{
          environment: { kind: 'wsl', distro_name: 'Ubuntu' },
          scope: { scope: 'project', project_id: 'source' },
        }}
        environments={[
          { environment: { kind: 'host' }, displayName: 'Windows', status: 'available', revision: 1, error: null },
          { environment: { kind: 'wsl', distro_name: 'Ubuntu' }, displayName: 'Ubuntu', status: 'available', revision: 1, error: null },
        ]}
        projectsByEnvironment={{
          host: [{
            binding: {
              id: 'host-target', nativePath: 'C:\\Code\\target', displayName: 'Host target',
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
        onLoadProjects={vi.fn(async () => undefined)}
        onClose={vi.fn()}
        onCopy={onCopy}
      />
    );

    fireEvent.click(screen.getByRole('combobox', { name: 'skills.copyToProject.targetEnvironment' }));
    fireEvent.click(await screen.findByRole('option', { name: 'Windows' }));
    fireEvent.click(await screen.findByText('Host target'));
    fireEvent.click(screen.getByRole('button', { name: 'skills.copyToProject.copy' }));

    await waitFor(() => {
      expect(onCopy).toHaveBeenCalledWith({
        environment: { kind: 'host' },
        projectIds: ['host-target'],
      });
      expect((screen.getByRole('button', {
        name: 'skills.copyToProject.copy',
      }) as HTMLButtonElement).disabled).toBe(false);
    });
  });

  it('keeps partial copy results in the dialog and excludes completed projects from retry', async () => {
    const onCopy = vi.fn(async () => ({
      status: 'partial' as const,
      response: { units: [] },
      succeededProjectIds: ['project-b'],
      failedProjectIds: ['project-c'],
      retryableProjectIds: ['project-c'],
    } satisfies CopyOutcome));
    const projects = [
      ...defaultCopyProps.projectsByEnvironment.host,
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
        projectsByEnvironment={{ host: projects }}
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

  it('shows a lightweight source note when source skill has incomplete update metadata', async () => {
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
    const note = screen.getByRole('note');
    expect(note.textContent).toContain('skills.copyToProject.metadataWarning');
    expect(note.querySelector('.text-warning')).toBeNull();
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
    expect(screen.queryByRole('button', { name: 'skills.copyToProject.repairSource' })).toBeNull();
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

  it('blocks copying and offers source repair when source information is missing', async () => {
    const onRepairSource = vi.fn();
    render(
      <CopyToProjectDialog
        skill={skill({
          source: null,
          sourceUrl: null,
          canRunUpdate: null,
          canCheckForUpdates: null,
          updateReason: null,
        })}
        {...defaultCopyProps}
        onClose={vi.fn()}
        onCopy={vi.fn()}
        onRepairSource={onRepairSource}
      />
    );

    await screen.findByText('/project-b');
    const note = screen.getByRole('note');
    expect(note.textContent).toContain('skills.copyToProject.sourceRepairRequired');
    const repair = screen.getByRole('button', { name: 'skills.copyToProject.repairSource' });
    expect(repair).toBeDefined();
    expect((screen.getByRole('button', {
      name: 'skills.copyToProject.copy',
    }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(repair);
    expect(onRepairSource).toHaveBeenCalledTimes(1);
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
    const hostPresence = deferred<Array<{ projectId: string; hasSkill: boolean }>>();
    const ubuntuPresence = deferred<Array<{ projectId: string; hasSkill: boolean }>>();
    const checkExistence = vi.fn((
      _skillName: string,
      environment: { kind: string },
    ) => environment.kind === 'host' ? hostPresence.promise : ubuntuPresence.promise);

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
    hostPresence.resolve([{ projectId: 'project-b', hasSkill: true }]);

    await waitFor(() => {
      expect(screen.queryByText('skills.copyToProject.installed')).toBeNull();
    });
  });
});
