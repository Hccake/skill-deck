/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ActiveMutation } from '@/bindings';
import { useMutationStore } from '@/stores/mutation';
import type { ProjectRemovalRequest } from '@/stores/project-removal';
import { RemoveProjectDialog } from '../RemoveProjectDialog';

const mocks = vi.hoisted(() => ({
  confirmProjectRemoval: vi.fn(),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock('@/stores/project-removal', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/stores/project-removal')>();
  return { ...original, confirmProjectRemoval: mocks.confirmProjectRemoval };
});

const request: ProjectRemovalRequest = {
  environment: { kind: 'wsl', distro_name: 'Ubuntu' },
  projectId: 'project-a',
  projectName: 'Project A',
  contextRevision: 3,
};
const activeMutation: ActiveMutation = {
  id: 'mutation-1',
  kind: 'install',
  context: { environment: { kind: 'native' }, scope: { scope: 'global' } },
  phase: 'preparing',
  progress: null,
  cancelable: true,
};

describe('RemoveProjectDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useMutationStore.setState({
      revision: 0,
      activeMutation: null,
      cancelling: false,
      loading: false,
    });
  });

  it('states that removal unregisters the project without deleting its directory', () => {
    render(<RemoveProjectDialog request={request} onClose={vi.fn()} />);

    expect(screen.getByText('context.removeConfirm.unregisterOnly')).toBeDefined();
  });

  it('keeps cancel available while another mutation blocks confirmation', () => {
    useMutationStore.setState({ activeMutation });

    render(<RemoveProjectDialog request={request} onClose={vi.fn()} />);

    expect((screen.getByRole('button', { name: 'context.removeConfirm.confirm' }) as HTMLButtonElement).disabled)
      .toBe(true);
    expect((screen.getByRole('button', { name: 'context.removeConfirm.cancel' }) as HTMLButtonElement).disabled)
      .toBe(false);
  });

  it('closes only after backend removal succeeds', async () => {
    let finishRemoval: ((removed: boolean) => void) | undefined;
    mocks.confirmProjectRemoval.mockImplementation(() => new Promise<boolean>((resolve) => {
      finishRemoval = resolve;
    }));
    const onClose = vi.fn();
    render(<RemoveProjectDialog request={request} onClose={onClose} />);

    fireEvent.click(screen.getByRole('button', { name: 'context.removeConfirm.confirm' }));
    expect(onClose).not.toHaveBeenCalled();
    finishRemoval?.(true);

    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(mocks.confirmProjectRemoval).toHaveBeenCalledWith(request);
  });

  it('shows removal progress and prevents implicit dismissal while the request is pending', async () => {
    mocks.confirmProjectRemoval.mockImplementation(() => new Promise<boolean>(() => undefined));
    const onClose = vi.fn();
    render(<RemoveProjectDialog request={request} onClose={onClose} />);

    fireEvent.click(screen.getByRole('button', { name: 'context.removeConfirm.confirm' }));

    expect((screen.getByRole('button', {
      name: 'context.removeConfirm.removing',
    }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(onClose).not.toHaveBeenCalled();
  });

  it('keeps a failed removal open and retries it in place', async () => {
    mocks.confirmProjectRemoval
      .mockRejectedValueOnce(new Error('remove failed'))
      .mockResolvedValueOnce(true);
    const onClose = vi.fn();
    render(<RemoveProjectDialog request={request} onClose={onClose} />);

    fireEvent.click(screen.getByRole('button', { name: 'context.removeConfirm.confirm' }));

    expect((await screen.findByRole('alert')).textContent)
      .toContain('context.removeConfirm.removeError');
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'context.removeConfirm.retry' }));

    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(mocks.confirmProjectRemoval).toHaveBeenCalledTimes(2);
  });

  it('reports the removed request only after backend removal succeeds', async () => {
    mocks.confirmProjectRemoval.mockResolvedValue(true);
    const onRemoved = vi.fn();
    render(<RemoveProjectDialog request={request} onClose={vi.fn()} onRemoved={onRemoved} />);

    fireEvent.click(screen.getByRole('button', { name: 'context.removeConfirm.confirm' }));

    await waitFor(() => expect(onRemoved).toHaveBeenCalledWith(request));
    expect(mocks.confirmProjectRemoval).toHaveBeenCalledWith(request);
  });

  it('keeps the dialog open without an error when removal was not executed', async () => {
    mocks.confirmProjectRemoval.mockResolvedValue(false);
    const onClose = vi.fn();
    render(<RemoveProjectDialog request={request} onClose={onClose} />);

    fireEvent.click(screen.getByRole('button', { name: 'context.removeConfirm.confirm' }));

    await waitFor(() => expect(mocks.confirmProjectRemoval).toHaveBeenCalledWith(request));
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.queryByRole('alert')).toBeNull();
  });
});
