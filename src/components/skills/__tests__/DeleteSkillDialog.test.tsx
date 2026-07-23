/* @vitest-environment jsdom */

import '@/test-utils';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useMutationStore } from '@/stores/mutation';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { DeleteSkillDialog } from '../DeleteSkillDialog';

const mocks = vi.hoisted(() => ({
  executeSkillRemoval: vi.fn(),
  openSkillRemoval: vi.fn(),
}));

vi.mock('@/workflows/skill-remove', () => ({
  executeSkillRemoval: (...args: unknown[]) => mocks.executeSkillRemoval(...args),
  openSkillRemoval: (...args: unknown[]) => mocks.openSkillRemoval(...args),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const context = { environment: { kind: 'host' }, scope: { scope: 'global' } } as const;
const canonicalPath = 'C:\\Users\\example\\.agents\\skills\\a-very-long-skill-name';
const agentPath = '/home/example/.custom-agent/skills/a-very-long-skill-name';

describe('DeleteSkillDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useMutationStore.setState({ activeMutation: null, loading: false, cancelling: false });
    useSkillDialogStore.setState({
      deleteTarget: {
        skill: { name: 'toolkit', canonicalPath } as never,
        scope: 'global',
        context,
      },
      loadingAgentDetails: false,
      deleteFeedback: null,
      deletePreview: {
        token: {} as never,
        context,
        skillName: 'toolkit',
        canonical: 'directory',
        physicalEntries: [
          {
            entryId: 'entry-copy',
            displayPath: { environment: { kind: 'host' }, nativePath: agentPath },
            kind: 'directory',
            physicalTargetKey: 'target-copy',
            owners: [{
              agentId: 'custom-agent',
              displayName: 'A Custom Agent With An Exceptionally Long Display Name',
              logicalTargetId: 'custom-agent',
            }],
            willBreakIfCanonicalRemoved: false,
          },
          {
            entryId: 'entry-link',
            displayPath: { environment: { kind: 'host' }, nativePath: '/agents/codex/toolkit' },
            kind: 'symlink',
            physicalTargetKey: 'target-link',
            owners: [{ agentId: 'codex', displayName: 'Codex', logicalTargetId: 'codex' }],
            willBreakIfCanonicalRemoved: false,
          },
        ],
      },
    });
  });

  it('shows the complete removal scope and submits one confirmation', async () => {
    render(<DeleteSkillDialog />);

    expect(screen.queryAllByRole('checkbox')).toHaveLength(0);
    expect(screen.getByText(canonicalPath)).not.toBeNull();
    expect(screen.getByText(agentPath)).not.toBeNull();
    expect(screen.getByText('skills.deleteConfirm.copyMode')).not.toBeNull();
    expect(screen.getByText('skills.deleteConfirm.linkMode')).not.toBeNull();
    expect(screen.queryByText('directory')).toBeNull();
    expect(screen.queryByText('symlink')).toBeNull();

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'skills.deleteConfirm.confirm' }));
    });
    expect(mocks.executeSkillRemoval).toHaveBeenCalledWith();
  });

  it('keeps long scope content inside a scrolling dialog body', () => {
    render(<DeleteSkillDialog />);

    const dialog = screen.getByRole('dialog');
    const body = screen.getByTestId('delete-skill-dialog-body');
    expect(dialog.className).toContain('min-w-0');
    expect(dialog.className).toContain('max-h-[calc(100dvh-2rem)]');
    expect(dialog.className).toContain('overflow-hidden');
    expect(body.className).toContain('min-w-0');
    expect(body.className).toContain('overflow-y-auto');
    expect(body.className).toContain('overflow-x-hidden');
    expect(body.className).toContain('overscroll-contain');
  });

  it('offers preview retry without closing the dialog', async () => {
    useSkillDialogStore.setState({
      deletePreview: null,
      deleteFeedback: 'previewError',
    });
    render(<DeleteSkillDialog />);

    expect(screen.getByText('skills.deleteConfirm.previewError')).not.toBeNull();
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'skills.deleteConfirm.retryPreview' }));
    });
    expect(mocks.openSkillRemoval).toHaveBeenCalledWith(
      expect.objectContaining({ name: 'toolkit' }),
      context,
      undefined,
    );
  });

  it('blocks confirmation during another mutation', () => {
    useMutationStore.setState({ activeMutation: { id: 'busy' } as never });
    render(<DeleteSkillDialog />);
    expect((screen.getByRole('button', {
      name: 'skills.deleteConfirm.confirm',
    }) as HTMLButtonElement).disabled).toBe(true);
  });
});
