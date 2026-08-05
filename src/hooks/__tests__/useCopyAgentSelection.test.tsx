/* @vitest-environment jsdom */

import '@/test-utils';
import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ContextRef, CopyAgentSelectionSnapshot } from '@/bindings';
import { makeAgentSelectionSnapshot } from '@/test-utils';

const mocks = vi.hoisted(() => ({
  getCopyAgentSelection: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  getCopyAgentSelection: (...args: unknown[]) => mocks.getCopyAgentSelection(...args),
}));

import { useCopyAgentSelection } from '../useCopyAgentSelection';

const source = (projectId: string): ContextRef => ({
  environment: { kind: 'host' },
  scope: { scope: 'project', project_id: projectId },
});

const snapshot = (revision: string): CopyAgentSelectionSnapshot => ({
  selection: makeAgentSelectionSnapshot({ revision }),
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

describe('useCopyAgentSelection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('loads the selection from the source Skill only', async () => {
    mocks.getCopyAgentSelection.mockResolvedValue(snapshot('selection-1'));

    const { result } = renderHook(() => useCopyAgentSelection(source('source'), 'toolkit'));

    await waitFor(() => expect(result.current.status).toBe('ready'));
    expect(mocks.getCopyAgentSelection).toHaveBeenCalledWith(source('source'), 'toolkit');
  });

  it('does not let an older source request replace the current selection', async () => {
    const first = deferred<CopyAgentSelectionSnapshot>();
    const second = deferred<CopyAgentSelectionSnapshot>();
    mocks.getCopyAgentSelection
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const { result, rerender } = renderHook(
      ({ projectId }) => useCopyAgentSelection(source(projectId), 'toolkit'),
      { initialProps: { projectId: 'first' } },
    );

    rerender({ projectId: 'second' });
    await act(async () => {
      second.resolve(snapshot('selection-2'));
      await second.promise;
    });
    await waitFor(() => expect(result.current.status).toBe('ready'));

    await act(async () => {
      first.resolve(snapshot('selection-1'));
      await first.promise;
    });
    expect(result.current.status).toBe('ready');
    if (result.current.status === 'ready') {
      expect(result.current.snapshot.selection.revision).toBe('selection-2');
    }
  });
});
