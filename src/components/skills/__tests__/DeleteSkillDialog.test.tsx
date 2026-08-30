/* @vitest-environment jsdom */

import '@/test-utils';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
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

vi.mock('@/components/recovery/RecoveryActions', () => ({
  RecoveryActions: ({ recovery, onResolved }: {
    recovery: { resourceId: string };
    onResolved?: () => void;
  }) => (
    <button type="button" onClick={onResolved}>recovery-actions:{recovery.resourceId}</button>
  ),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const context = { environment: { kind: 'native' }, scope: { scope: 'global' } } as const;
const basePath = 'D:\\Code\\temp\\skills';
const canonicalPath = `${basePath}\\.agents\\skills\\a-very-long-skill-name`;
const agentPath = `${basePath}\\.custom-agent\\skills\\a-very-long-skill-name`;

describe('DeleteSkillDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.executeSkillRemoval.mockResolvedValue({ status: 'succeeded' });
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
        standard: 'directory',
        physicalEntries: [
          {
            entryId: 'entry-copy',
            displayPath: { environment: { kind: 'native' }, nativePath: agentPath },
            kind: 'directory',
            physicalTargetKey: 'target-copy',
            readers: [{
              agentId: 'custom-agent',
              displayName: 'A Custom Agent With An Exceptionally Long Display Name',
              logicalTargetId: 'custom-agent',
            }],
            willBreakIfStandardRemoved: false,
          },
          {
            entryId: 'entry-link',
            displayPath: { environment: { kind: 'native' }, nativePath: '/agents/codex/toolkit' },
            kind: 'symlink',
            physicalTargetKey: 'target-link',
            readers: [{ agentId: 'codex', displayName: 'Codex', logicalTargetId: 'codex' }],
            willBreakIfStandardRemoved: false,
          },
        ],
        restoresLibrary: false,
      },
    });
  });

  it('shows the complete removal scope and submits one confirmation', async () => {
    render(<DeleteSkillDialog />);

    expect(screen.queryAllByRole('checkbox')).toHaveLength(0);
    expect(screen.queryByText(basePath)).toBeNull();
    expect(screen.getByText('.agents\\skills\\a-very-long-skill-name')).not.toBeNull();
    expect(screen.getByText('.custom-agent\\skills\\a-very-long-skill-name')).not.toBeNull();
    expect(screen.queryByText(canonicalPath)).toBeNull();
    expect(screen.queryByText(agentPath)).toBeNull();
    expect(screen.getByText('skills.deleteConfirm.copyMode')).not.toBeNull();
    expect(screen.getByText('skills.deleteConfirm.linkMode')).not.toBeNull();
    expect(screen.queryByText('directory')).toBeNull();
    expect(screen.queryByText('symlink')).toBeNull();

    const relativePaths = screen.getByRole('button', {
      name: 'skills.deleteConfirm.relativePaths',
    });
    const fullPaths = screen.getByRole('button', {
      name: 'skills.deleteConfirm.fullPaths',
    });
    expect(relativePaths.getAttribute('aria-pressed')).toBe('true');
    expect(fullPaths.getAttribute('aria-pressed')).toBe('false');

    fireEvent.click(fullPaths);
    expect(screen.getByText(canonicalPath)).not.toBeNull();
    expect(screen.getByText(agentPath)).not.toBeNull();
    expect(relativePaths.getAttribute('aria-pressed')).toBe('false');
    expect(fullPaths.getAttribute('aria-pressed')).toBe('true');

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'skills.deleteConfirm.confirm' }));
    });
    expect(mocks.executeSkillRemoval).toHaveBeenCalledWith();
  });

  it('does not add a redundant message when there are no Agent Skill directories', () => {
    const preview = useSkillDialogStore.getState().deletePreview!;
    useSkillDialogStore.setState({
      deletePreview: { ...preview, physicalEntries: [] },
    });

    render(<DeleteSkillDialog />);

    expect(screen.queryByText('skills.deleteConfirm.noAgentEntries')).toBeNull();
    expect(screen.getAllByTestId('delete-skill-entry')).toHaveLength(1);
  });

  it('explains that deleting the direct installation restores the Library winner', () => {
    const preview = useSkillDialogStore.getState().deletePreview!;
    useSkillDialogStore.setState({
      deletePreview: { ...preview, restoresLibrary: true },
    });

    render(<DeleteSkillDialog />);

    expect(screen.getByText('skills.deleteConfirm.restoresLibrary')).not.toBeNull();
  });

  it('focuses the safe action when the destructive dialog opens', async () => {
    render(<DeleteSkillDialog />);

    const cancel = screen.getByRole('button', { name: 'common.cancel' });
    await waitFor(() => expect(document.activeElement).toBe(cancel));
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

  it('shows recovery actions and removes ordinary delete retry after recovery is required', async () => {
    mocks.executeSkillRemoval.mockResolvedValueOnce({
      status: 'recoveryRequired',
      recovery: [{ resourceId: 'recovery-1', suggestedActionCode: 'reviewChanges' }],
    });
    render(<DeleteSkillDialog />);

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'skills.deleteConfirm.confirm' }));
    });

    expect(screen.getByText('skills.deleteConfirm.recoveryRequired')).not.toBeNull();
    expect(screen.getByText('recovery-actions:recovery-1')).not.toBeNull();
    expect(screen.queryByRole('button', { name: 'skills.deleteConfirm.confirm' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'skills.deleteConfirm.retryDelete' })).toBeNull();
  });

  it('closes the dialog after Backend confirms that recovery is resolved', async () => {
    mocks.executeSkillRemoval.mockResolvedValueOnce({
      status: 'recoveryRequired',
      recovery: [{ resourceId: 'recovery-1', suggestedActionCode: 'reviewChanges' }],
    });
    render(<DeleteSkillDialog />);

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'skills.deleteConfirm.confirm' }));
    });
    fireEvent.click(screen.getByRole('button', { name: 'recovery-actions:recovery-1' }));

    expect(useSkillDialogStore.getState().deleteTarget).toBeNull();
  });

  it('blocks confirmation during another mutation', () => {
    useMutationStore.setState({ activeMutation: { id: 'busy' } as never });
    render(<DeleteSkillDialog />);
    expect((screen.getByRole('button', {
      name: 'skills.deleteConfirm.confirm',
    }) as HTMLButtonElement).disabled).toBe(true);
  });
});
