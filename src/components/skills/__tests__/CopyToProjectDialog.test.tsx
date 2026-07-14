/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
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

describe('CopyToProjectDialog', () => {
  beforeEach(() => {
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
  });

  it('disables copying while another mutation is active', () => {
    useMutationStore.setState({
      activeMutation: {
        kind: 'update',
        context: { environment: { kind: 'host' }, scope: { scope: 'global' } },
        statusText: 'Updating',
        cancelable: true,
      },
    });
    render(
      <CopyToProjectDialog
        skill={skill()}
        currentProjectPath="/project-a"
        projects={['/project-a', '/project-b']}
        onClose={vi.fn()}
        onCopy={vi.fn()}
      />
    );

    fireEvent.click(screen.getByText('/project-b'));
    expect((screen.getByRole('button', { name: 'skills.copyToProject.copy' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('does not show a source note when copied skill can keep update metadata', () => {
    render(
      <CopyToProjectDialog
        skill={skill()}
        currentProjectPath="/project-a"
        projects={['/project-a', '/project-b']}
        onClose={vi.fn()}
        onCopy={vi.fn()}
      />
    );

    expect(screen.queryByText('skills.copyToProject.metadataWarning')).toBeNull();
  });

  it('shows a lightweight source note when source skill has incomplete update metadata', () => {
    render(
      <CopyToProjectDialog
        skill={skill({
          canRunUpdate: true,
          canCheckForUpdates: false,
          updateReason: 'missing-remote-hash',
        })}
        currentProjectPath="/project-a"
        projects={['/project-a', '/project-b']}
        onClose={vi.fn()}
        onCopy={vi.fn()}
      />
    );

    const note = screen.getByRole('note');
    expect(note.textContent).toContain('skills.copyToProject.metadataWarning');
    expect(note.querySelector('.text-warning')).toBeNull();
  });

  it('does not show the source note for temporary update check failures', () => {
    render(
      <CopyToProjectDialog
        skill={skill({
          canRunUpdate: true,
          canCheckForUpdates: false,
          updateReason: 'network-error',
        })}
        currentProjectPath="/project-a"
        projects={['/project-a', '/project-b']}
        onClose={vi.fn()}
        onCopy={vi.fn()}
      />
    );

    expect(screen.queryByText('skills.copyToProject.metadataWarning')).toBeNull();
  });

  it('shows a lightweight source note when source information is missing', () => {
    render(
      <CopyToProjectDialog
        skill={skill({
          source: null,
          sourceUrl: null,
          canRunUpdate: null,
          canCheckForUpdates: null,
          updateReason: null,
        })}
        currentProjectPath="/project-a"
        projects={['/project-a', '/project-b']}
        onClose={vi.fn()}
        onCopy={vi.fn()}
      />
    );

    const note = screen.getByRole('note');
    expect(note.textContent).toContain('skills.copyToProject.metadataWarning');
    expect(note.querySelector('.text-warning')).toBeNull();
  });
});
