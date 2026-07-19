/* @vitest-environment jsdom */

import '@/test-utils';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useMutationStore } from '@/stores/mutation';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { DeleteSkillDialog } from '../DeleteSkillDialog';

const mocks = vi.hoisted(() => ({ executeSkillRemoval: vi.fn() }));

vi.mock('@/workflows/skill-remove', () => ({
  executeSkillRemoval: (...args: unknown[]) => mocks.executeSkillRemoval(...args),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const context = { environment: { kind: 'host' }, scope: { scope: 'global' } } as const;

describe('DeleteSkillDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useMutationStore.setState({ activeMutation: null, loading: false, cancelling: false });
    useSkillDialogStore.setState({
      deleteTarget: {
        skill: { name: 'toolkit' } as never,
        scope: 'global',
        context,
      },
      loadingAgentDetails: false,
      deletePreview: {
        token: {} as never,
        context,
        skillName: 'toolkit',
        canonical: 'directory',
        physicalEntries: [{
          entryId: 'entry-1',
          displayPath: { environment: { kind: 'host' }, nativePath: '/agents/codex/toolkit' },
          kind: 'directory',
          physicalTargetKey: 'target-1',
          owners: [{ agentId: 'codex', displayName: 'Codex', logicalTargetId: 'codex' }],
          willBreakIfCanonicalRemoved: false,
        }],
      },
    });
  });

  it('submits observed entry IDs and explicit directory confirmation', async () => {
    render(<DeleteSkillDialog />);
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'skills.deleteConfirm.confirm' }));
    });
    expect(mocks.executeSkillRemoval).toHaveBeenCalledWith({
      removeCanonical: false,
      entryIds: ['entry-1'],
      confirmEntityDirectories: true,
    });
  });

  it('allows canonical-only removal without selecting a physical Agent entry', async () => {
    render(<DeleteSkillDialog />);
    fireEvent.click(screen.getByRole('checkbox', { name: /skills.deleteConfirm.removeCanonical/ }));
    fireEvent.click(screen.getByRole('checkbox', { name: /Codex/ }));
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'skills.deleteConfirm.confirm' }));
    });
    expect(mocks.executeSkillRemoval).toHaveBeenCalledWith({
      removeCanonical: true,
      entryIds: [],
      confirmEntityDirectories: false,
    });
  });

  it('blocks confirmation during another mutation', () => {
    useMutationStore.setState({ activeMutation: { id: 'busy' } as never });
    render(<DeleteSkillDialog />);
    expect((screen.getByRole('button', {
      name: 'skills.deleteConfirm.confirm',
    }) as HTMLButtonElement).disabled).toBe(true);
  });
});
