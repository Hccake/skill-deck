import { beforeEach, describe, expect, it, vi } from 'vitest';
import { libraryWorkspace } from '@/lib/libraries/workspace';
import {
  captureLibraryDeletion,
  confirmLibraryDeletion,
} from '../library-deletion';

describe('Library deletion workflow', () => {
  beforeEach(() => vi.restoreAllMocks());

  it('captures and confirms the clicked Library instead of the current selection', async () => {
    const request = captureLibraryDeletion(
      { kind: 'wsl', distro_name: 'Ubuntu' },
      { id: 'lib-b', name: 'Backend', skillCount: 4 },
    );
    const execute = vi.spyOn(libraryWorkspace, 'execute').mockResolvedValue({
      status: 'succeeded',
      snapshot: libraryWorkspace.getSnapshot(request.environment),
    });

    const result = await confirmLibraryDeletion(request);

    expect(request).toEqual({
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      libraryId: 'lib-b',
      libraryName: 'Backend',
      skillCount: 4,
    });
    expect(execute).toHaveBeenCalledWith({
      kind: 'delete',
      environment: request.environment,
      libraryId: 'lib-b',
    });
    expect(result.status).toBe('deleted');
  });

  it('refreshes and reports a stale target when the Library no longer exists', async () => {
    const request = captureLibraryDeletion(
      { kind: 'native' },
      { id: 'missing', name: 'Missing', skillCount: 0 },
    );
    const execute = vi.spyOn(libraryWorkspace, 'execute')
      .mockResolvedValueOnce({
        status: 'failed',
        failureSource: 'command',
        error: { kind: 'pathNotFound', data: { path: 'missing' } },
        snapshot: libraryWorkspace.getSnapshot(request.environment),
      })
      .mockResolvedValueOnce({
        status: 'succeeded',
        snapshot: libraryWorkspace.getSnapshot(request.environment),
      });

    const result = await confirmLibraryDeletion(request);

    expect(result).toEqual({ status: 'stale', request });
    expect(execute).toHaveBeenLastCalledWith({ kind: 'load', environment: request.environment });
  });

  it('keeps the dialog retryable when refreshing a stale target fails', async () => {
    const request = captureLibraryDeletion(
      { kind: 'native' },
      { id: 'missing', name: 'Missing', skillCount: 0 },
    );
    vi.spyOn(libraryWorkspace, 'execute')
      .mockResolvedValueOnce({
        status: 'failed',
        failureSource: 'command',
        error: { kind: 'pathNotFound', data: { path: 'missing' } },
        snapshot: libraryWorkspace.getSnapshot(request.environment),
      })
      .mockResolvedValueOnce({
        status: 'failed',
        failureSource: 'catalog',
        error: { kind: 'io', data: { message: 'refresh failed' } },
        snapshot: libraryWorkspace.getSnapshot(request.environment),
      });

    const result = await confirmLibraryDeletion(request);

    expect(result).toEqual({
      status: 'failed',
      request,
      error: { kind: 'io', data: { message: 'refresh failed' } },
    });
  });
});
