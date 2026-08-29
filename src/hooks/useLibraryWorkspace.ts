import { useCallback, useSyncExternalStore } from 'react';
import type { EnvironmentRef } from '@/bindings';
import {
  libraryWorkspace,
  type LibraryWorkspaceCommand,
  type LibraryWorkspaceInput,
  type LibraryWorkspaceResult,
  type LibraryWorkspaceState,
} from '@/lib/libraries/workspace';

export function useLibraryWorkspace(environment: EnvironmentRef): LibraryWorkspaceState & {
  execute: (command: LibraryWorkspaceInput) => Promise<LibraryWorkspaceResult>;
} {
  const snapshot = useSyncExternalStore(
    libraryWorkspace.subscribe,
    () => libraryWorkspace.getSnapshot(environment),
    () => libraryWorkspace.getSnapshot(environment),
  );
  const execute = useCallback(
    (command: Omit<LibraryWorkspaceCommand, 'environment'>) => (
      libraryWorkspace.execute({ ...command, environment } as LibraryWorkspaceCommand)
    ),
    [environment],
  );
  return { ...snapshot, execute };
}
