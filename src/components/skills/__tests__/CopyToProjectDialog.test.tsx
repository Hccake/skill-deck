/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { CopyToProjectDialog } from '../CopyToProjectDialog';
import type { InstalledSkill } from '@/bindings';
import { useMutationStore } from '@/stores/mutation';

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
    const onCopy = vi.fn(async () => undefined);
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

  it('shows a lightweight source note when source information is missing', async () => {
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
      />
    );

    await screen.findByText('/project-b');
    const note = screen.getByRole('note');
    expect(note.textContent).toContain('skills.copyToProject.metadataWarning');
    expect(note.querySelector('.text-warning')).toBeNull();
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
