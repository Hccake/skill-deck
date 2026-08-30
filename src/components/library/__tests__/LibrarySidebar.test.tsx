/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { LibrarySidebar } from '../LibrarySidebar';

const mocks = vi.hoisted(() => ({
  toastInfo: vi.fn(),
}));

vi.mock('sonner', () => ({
  toast: { info: mocks.toastInfo },
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) => (
      values ? `${key}:${JSON.stringify(values)}` : key
    ),
  }),
}));

function renderSidebar({
  confirmedCount = 0,
  pendingCount = 0,
  busy = false,
}: {
  confirmedCount?: number;
  pendingCount?: number;
  busy?: boolean;
} = {}) {
  const onDeleteLibrary = vi.fn();

  render(
    <TooltipProvider>
      <LibrarySidebar
        libraries={[{ id: 'lib-1', name: 'Backend', skillCount: 4 }]}
        usageProjection={confirmedCount > 0 || pendingCount > 0 ? [{
          libraryId: 'lib-1',
          confirmedCount,
          pendingCount,
        }] : []}
        selectedLibraryId="lib-1"
        busy={busy}
        onSelectLibrary={vi.fn()}
        onCreateLibrary={vi.fn()}
        onRenameLibrary={vi.fn()}
        onDeleteLibrary={onDeleteLibrary}
      />
    </TooltipProvider>,
  );

  return {
    deleteButton: screen.getByRole('button', {
      name: 'libraries.deleteNamed:{"name":"Backend"}',
    }),
    onDeleteLibrary,
  };
}

describe('LibrarySidebar', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it.each([
    {
      counts: { confirmedCount: 1, pendingCount: 0 },
      reason: 'libraries.lockedDeleteApplied',
    },
    {
      counts: { confirmedCount: 0, pendingCount: 1 },
      reason: 'libraries.lockedDeletePending',
    },
    {
      counts: { confirmedCount: 1, pendingCount: 1 },
      reason: 'libraries.lockedDeleteAppliedWithPending',
    },
  ])('explains why a locked Library cannot be deleted: $reason', async ({ counts, reason }) => {
    const { deleteButton, onDeleteLibrary } = renderSidebar(counts);

    expect(deleteButton.getAttribute('aria-disabled')).toBe('true');

    fireEvent.focus(deleteButton);
    expect((await screen.findByRole('tooltip')).textContent).toBe(reason);

    fireEvent.click(deleteButton);
    expect(mocks.toastInfo).toHaveBeenCalledWith(reason);
    expect(onDeleteLibrary).not.toHaveBeenCalled();
  });

  it('starts deletion for a Library that is not in use', () => {
    const { deleteButton, onDeleteLibrary } = renderSidebar();

    expect(deleteButton.getAttribute('aria-disabled')).toBeNull();
    fireEvent.click(deleteButton);

    expect(onDeleteLibrary).toHaveBeenCalledTimes(1);
    expect(mocks.toastInfo).not.toHaveBeenCalled();
  });

  it('uses native disabled semantics while another Library operation is running', () => {
    const { deleteButton, onDeleteLibrary } = renderSidebar({ busy: true });

    expect((deleteButton as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(deleteButton);

    expect(onDeleteLibrary).not.toHaveBeenCalled();
    expect(mocks.toastInfo).not.toHaveBeenCalled();
  });
});
