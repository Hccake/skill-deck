import type {
  AppError,
  EnvironmentRef,
  LibraryId,
  SkillLibrarySummary,
} from '@/bindings';
import {
  libraryWorkspace,
  type LibraryWorkspaceState,
} from '@/lib/libraries/workspace';

export interface LibraryDeletionRequest {
  environment: EnvironmentRef;
  libraryId: LibraryId;
  libraryName: string;
  skillCount: number;
}

export type LibraryDeletionResult =
  | {
      status: 'deleted';
      request: LibraryDeletionRequest;
      snapshot: LibraryWorkspaceState;
    }
  | {
      status: 'failed';
      request: LibraryDeletionRequest;
      error: AppError;
    }
  | {
      status: 'notRun';
      request: LibraryDeletionRequest;
      reason: 'writeBlocked';
    }
  | { status: 'stale'; request: LibraryDeletionRequest };

export function captureLibraryDeletion(
  environment: EnvironmentRef,
  library: SkillLibrarySummary,
): LibraryDeletionRequest {
  return {
    environment: { ...environment },
    libraryId: library.id,
    libraryName: library.name,
    skillCount: library.skillCount,
  };
}

export async function confirmLibraryDeletion(
  request: LibraryDeletionRequest,
): Promise<LibraryDeletionResult> {
  const result = await libraryWorkspace.execute({
    kind: 'delete',
    environment: request.environment,
    libraryId: request.libraryId,
  });
  if (result.status === 'failed') {
    if (result.error.kind === 'pathNotFound') {
      const refreshed = await libraryWorkspace.execute({
        kind: 'load',
        environment: request.environment,
      });
      if (refreshed.status === 'failed') {
        return { status: 'failed', request, error: refreshed.error };
      }
      return { status: 'stale', request };
    }
    return { status: 'failed', request, error: result.error };
  }
  if (result.status === 'notRun') {
    return { status: 'notRun', request, reason: result.reason };
  }
  return { status: 'deleted', request, snapshot: result.snapshot };
}
