/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { DeleteLibraryDialog } from '../DeleteLibraryDialog';
import type { LibraryDeletionRequest } from '@/workflows/library-deletion';
import { confirmLibraryDeletion } from '@/workflows/library-deletion';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) => (
      values ? `${key}:${JSON.stringify(values)}` : key
    ),
  }),
}));

vi.mock('@/utils/format-app-error', () => ({
  formatAppError: () => 'formatted-delete-error',
}));

vi.mock('@/workflows/library-deletion', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/workflows/library-deletion')>();
  return { ...actual, confirmLibraryDeletion: vi.fn() };
});

const request: LibraryDeletionRequest = {
  environment: { kind: 'native' },
  libraryId: 'lib-b',
  libraryName: 'Backend',
  skillCount: 4,
};

describe('DeleteLibraryDialog', () => {
  it('keeps the captured target open after failure and retries the same request', async () => {
    vi.mocked(confirmLibraryDeletion)
      .mockResolvedValueOnce({
        status: 'failed',
        request,
        error: { kind: 'staleTarget' },
      })
      .mockResolvedValueOnce({
        status: 'deleted',
        request,
        snapshot: {} as never,
      });
    const onClose = vi.fn();
    render(<DeleteLibraryDialog request={request} onClose={onClose} />);

    expect(screen.getByText('libraries.deleteLibraryTitle:{"name":"Backend"}')).toBeTruthy();
    expect(screen.getByText('libraries.deleteLibraryDescriptionWithCount:{"count":4}')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'common.delete' }));

    expect((await screen.findByRole('alert')).textContent).toBe('formatted-delete-error');
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'libraries.retryDeleteLibrary' }));

    await waitFor(() => expect(confirmLibraryDeletion).toHaveBeenCalledTimes(2));
    expect(confirmLibraryDeletion).toHaveBeenNthCalledWith(1, request);
    expect(confirmLibraryDeletion).toHaveBeenNthCalledWith(2, request);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('cannot be dismissed while deletion is submitting', async () => {
    let resolveDeletion!: (value: Awaited<ReturnType<typeof confirmLibraryDeletion>>) => void;
    vi.mocked(confirmLibraryDeletion).mockReturnValue(new Promise((resolve) => {
      resolveDeletion = resolve;
    }));
    const onClose = vi.fn();
    render(<DeleteLibraryDialog request={request} onClose={onClose} />);

    fireEvent.click(screen.getByRole('button', { name: 'common.delete' }));

    expect((screen.getByRole('button', { name: 'common.cancel' }) as HTMLButtonElement).disabled)
      .toBe(true);
    expect(screen.getByRole('alertdialog').getAttribute('aria-busy')).toBe('true');
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(onClose).not.toHaveBeenCalled();

    resolveDeletion({ status: 'deleted', request, snapshot: {} as never });
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it('maps the backend usage validation to the localized locked message', async () => {
    vi.mocked(confirmLibraryDeletion).mockResolvedValue({
      status: 'failed',
      request,
      error: {
        kind: 'validation',
        data: { field: 'libraryId', message: 'internal validation message' },
      },
    });
    render(<DeleteLibraryDialog request={request} onClose={vi.fn()} />);

    fireEvent.click(screen.getByRole('button', { name: 'common.delete' }));

    expect((await screen.findByRole('alert')).textContent).toBe('libraries.lockedDelete');
  });
});
