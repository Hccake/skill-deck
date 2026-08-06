import * as tauriApi from '@/hooks/useTauriApi';
import type { EnvironmentRef } from '@/bindings';
import { environmentKey, globalContext, sameEnvironment } from '@/lib/context';
import {
  createProjectWorkspace,
  type ProjectCatalogObserver,
  type ProjectWorkspaceSnapshot,
} from '@/lib/projects/workspace';
import { isBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import { runBusinessWrite } from '@/workflows/install-session-feedback';
import { useEnvironmentStore } from './environment';

const HOST: EnvironmentRef = { kind: 'host' };

let catalogObserver: ProjectCatalogObserver = {
  captureContext: () => ({ context: globalContext(HOST), revision: 0 }),
  onCompleteSnapshot: () => undefined,
};

export function registerProjectCatalogObserver(observer: ProjectCatalogObserver): void {
  catalogObserver = observer;
}

function environmentIsAvailable(environment: EnvironmentRef): boolean {
  if (environment.kind === 'host') return true;
  return useEnvironmentStore.getState().environments.some((entry) => (
    sameEnvironment(entry.environment, environment) && entry.status === 'available'
  ));
}

function environmentRevision(environment: EnvironmentRef): number {
  const state = useEnvironmentStore.getState();
  const key = environmentKey(environment);
  return state.runtimeByEnvironment[key]?.revision
    ?? state.environments.find((entry) => environmentKey(entry.environment) === key)?.revision
    ?? 0;
}

export const projectWorkspace = createProjectWorkspace({
  backend: {
    list: (environment) => tauriApi.listEnvironmentProjects(environment),
    add: (environment, nativePath) => tauriApi.addEnvironmentProject(environment, nativePath),
    remove: (environment, projectId) => tauriApi.removeEnvironmentProject(environment, projectId),
    setCrossStorageWarning: (environment, projectId, suppressed) => (
      tauriApi.setEnvironmentProjectCrossStorageWarning(environment, projectId, suppressed)
    ),
  },
  environment: {
    isAvailable: environmentIsAvailable,
    revision: environmentRevision,
    ensureAvailable: async (environment) => {
      if (environmentIsAvailable(environment)) return;
      await useEnvironmentStore.getState().connect(environment);
    },
  },
  catalogObserver: {
    captureContext: () => catalogObserver.captureContext(),
    onCompleteSnapshot: (completion) => catalogObserver.onCompleteSnapshot(completion),
  },
  write: {
    run: async (operation) => {
      if (isBusinessWriteBlocked()) return { status: 'notRun' };
      const outcome = await runBusinessWrite(operation);
      return outcome.status === 'completed'
        ? { status: 'succeeded', value: outcome.value }
        : { status: 'notRun' };
    },
  },
});

export function projectSnapshotFor(environment: EnvironmentRef): ProjectWorkspaceSnapshot {
  return projectWorkspace.getSnapshot(environment);
}
