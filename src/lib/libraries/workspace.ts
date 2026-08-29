import type {
  AppError,
  EnvironmentRef,
  FetchResult,
  LibraryAddPreview,
  LibraryAddSkillResult,
  LibraryId,
  LibraryWorkspaceSnapshot,
  PreviewAddLibrarySkillsRequest,
  SkillLibraryDetail,
} from '@/bindings';
import {
  acquireSelectedPayloads,
  addSkillsToLibrary,
  createSkillLibrary,
  deleteSkillLibrary,
  getSkillLibrary,
  listSkillLibraries,
  previewAddLibrarySkills,
  renameSkillLibrary,
} from '@/hooks/useTauriApi';
import { environmentKey } from '@/lib/context';
import { toAppError } from '@/utils/to-app-error';
import { runBusinessWrite } from '@/workflows/install-session-feedback';

export type LibraryWorkspacePhase = 'idle' | 'loading' | 'ready' | 'writing' | 'error';
export type LibraryDetailPhase = 'idle' | 'loading' | 'ready' | 'error';

export interface LibraryWorkspaceState {
  environment: EnvironmentRef;
  phase: LibraryWorkspacePhase;
  catalog: LibraryWorkspaceSnapshot | null;
  selectedLibraryId: LibraryId | null;
  detail: SkillLibraryDetail | null;
  detailPhase: LibraryDetailPhase;
  detailError: AppError | null;
  catalogError: AppError | null;
  pendingAdd: { request: PreviewAddLibrarySkillsRequest; preview: LibraryAddPreview } | null;
  retryAdd: { request: PreviewAddLibrarySkillsRequest; error: AppError } | null;
  lastAddResults: LibraryAddSkillResult[];
  version: number;
}

export type LibraryWorkspaceResult =
  | { status: 'succeeded'; snapshot: LibraryWorkspaceState }
  | {
      status: 'failed';
      failureSource: 'catalog' | 'command';
      error: AppError;
      snapshot: LibraryWorkspaceState;
    }
  | { status: 'notRun'; reason: 'writeBlocked' };

export type LibraryWorkspaceCommand =
  | { kind: 'load'; environment: EnvironmentRef }
  | { kind: 'select'; environment: EnvironmentRef; libraryId: LibraryId }
  | { kind: 'create'; environment: EnvironmentRef; name: string }
  | { kind: 'rename'; environment: EnvironmentRef; libraryId: LibraryId; name: string }
  | { kind: 'delete'; environment: EnvironmentRef; libraryId: LibraryId }
  | {
    kind: 'addSkills';
    environment: EnvironmentRef;
    libraryId: LibraryId;
    discovery: FetchResult;
    skillPaths?: readonly string[];
  }
  | { kind: 'confirmAddSkills'; environment: EnvironmentRef; acknowledgeRedirect: boolean }
  | { kind: 'retryAddPreview'; environment: EnvironmentRef }
  | { kind: 'discardAddSkills'; environment: EnvironmentRef };

export type LibraryWorkspaceInput = LibraryWorkspaceCommand extends infer Command
  ? Command extends { environment: EnvironmentRef }
    ? Omit<Command, 'environment'>
    : never
  : never;

export interface LibraryWorkspace {
  getSnapshot(environment: EnvironmentRef): LibraryWorkspaceState;
  subscribe(listener: () => void): () => void;
  execute(command: LibraryWorkspaceCommand): Promise<LibraryWorkspaceResult>;
}

function emptyState(environment: EnvironmentRef): LibraryWorkspaceState {
  return {
    environment,
    phase: 'idle',
    catalog: null,
    selectedLibraryId: null,
    detail: null,
    detailPhase: 'idle',
    detailError: null,
    catalogError: null,
    pendingAdd: null,
    retryAdd: null,
    lastAddResults: [],
    version: 0,
  };
}

export function createLibraryWorkspace(): LibraryWorkspace {
  const states = new Map<string, LibraryWorkspaceState>();
  const generations = new Map<string, number>();
  const listeners = new Set<() => void>();

  const getSnapshot = (environment: EnvironmentRef) => {
    const key = environmentKey(environment);
    const existing = states.get(key);
    if (existing) return existing;
    const state = emptyState(environment);
    states.set(key, state);
    return state;
  };

  const commit = (
    environment: EnvironmentRef,
    update: (current: LibraryWorkspaceState) => Omit<LibraryWorkspaceState, 'version'>,
  ) => {
    const key = environmentKey(environment);
    const current = getSnapshot(environment);
    const next = { ...update(current), version: current.version + 1 };
    states.set(key, next);
    listeners.forEach((listener) => listener());
    return next;
  };

  const start = (environment: EnvironmentRef, phase: LibraryWorkspacePhase) => {
    const key = environmentKey(environment);
    const generation = (generations.get(key) ?? 0) + 1;
    generations.set(key, generation);
    commit(environment, (current) => ({ ...current, phase, catalogError: null }));
    return generation;
  };

  const currentGeneration = (environment: EnvironmentRef, generation: number) => (
    generations.get(environmentKey(environment)) === generation
  );

  const selectFromCatalog = async (
    environment: EnvironmentRef,
    catalog: LibraryWorkspaceSnapshot,
    preferred: LibraryId | null,
    generation: number,
  ) => {
    const selected = preferred && catalog.libraries.some((library) => library.id === preferred)
      ? preferred
      : catalog.libraries[0]?.id ?? null;
    let detail: SkillLibraryDetail | null = null;
    let detailPhase: LibraryDetailPhase = selected ? 'loading' : 'idle';
    let detailError: AppError | null = null;
    if (selected) {
      try {
        detail = await getSkillLibrary(environment, selected);
        detailPhase = 'ready';
      } catch (error) {
        detailPhase = 'error';
        detailError = toAppError(error);
      }
    }
    if (!currentGeneration(environment, generation)) return getSnapshot(environment);
    return commit(environment, (current) => ({
      ...current,
      phase: 'ready',
      catalog,
      selectedLibraryId: selected,
      detail,
      detailPhase,
      detailError,
      catalogError: null,
    }));
  };

  const catalogFailed = (environment: EnvironmentRef, error: unknown): LibraryWorkspaceResult => {
    const appError = toAppError(error);
    const snapshot = commit(environment, (current) => ({
      ...current,
      phase: 'error',
      catalogError: appError,
    }));
    return { status: 'failed', failureSource: 'catalog', error: appError, snapshot };
  };

  const commandFailed = (
    environment: EnvironmentRef,
    error: unknown,
    before: LibraryWorkspaceState,
  ): LibraryWorkspaceResult => {
    const appError = toAppError(error);
    const snapshot = commit(environment, () => ({
      ...before,
      phase: before.catalog ? 'ready' : 'idle',
      catalogError: before.catalogError,
    }));
    return { status: 'failed', failureSource: 'command', error: appError, snapshot };
  };

  const selectedAfterDelete = (
    before: LibraryWorkspaceState,
    catalog: LibraryWorkspaceSnapshot,
    deletedLibraryId: LibraryId,
  ): LibraryId | null => {
    const selected = before.selectedLibraryId;
    if (
      selected
      && selected !== deletedLibraryId
      && catalog.libraries.some((library) => library.id === selected)
    ) return selected;

    if (selected === deletedLibraryId) {
      const deletedIndex = before.catalog?.libraries.findIndex(
        (library) => library.id === deletedLibraryId,
      ) ?? -1;
      if (deletedIndex >= 0) {
        const next = before.catalog?.libraries[deletedIndex + 1]?.id;
        if (next && catalog.libraries.some((library) => library.id === next)) return next;
        const previous = before.catalog?.libraries[deletedIndex - 1]?.id;
        if (previous && catalog.libraries.some((library) => library.id === previous)) return previous;
      }
    }

    return catalog.libraries[0]?.id ?? null;
  };

  return {
    getSnapshot,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    async execute(command) {
      const { environment } = command;
      const before = getSnapshot(environment);
      const generation = start(
        environment,
        command.kind === 'load' || command.kind === 'select' ? 'loading' : 'writing',
      );
      try {
        if (command.kind === 'load') {
          const catalog = await listSkillLibraries(environment);
          const snapshot = await selectFromCatalog(
            environment,
            catalog,
            getSnapshot(environment).selectedLibraryId,
            generation,
          );
          return { status: 'succeeded', snapshot };
        }
        if (command.kind === 'select') {
          let detail: SkillLibraryDetail | null = null;
          let detailError: AppError | null = null;
          try {
            detail = await getSkillLibrary(environment, command.libraryId);
          } catch (error) {
            detailError = toAppError(error);
          }
          if (!currentGeneration(environment, generation)) {
            return { status: 'succeeded', snapshot: getSnapshot(environment) };
          }
          const snapshot = commit(environment, (current) => ({
            ...current,
            phase: 'ready',
            selectedLibraryId: command.libraryId,
            detail,
            detailPhase: detailError ? 'error' : 'ready',
            detailError,
            catalogError: null,
          }));
          return { status: 'succeeded', snapshot };
        }
        if (command.kind === 'delete') {
          const outcome = await runBusinessWrite(() => (
            deleteSkillLibrary(environment, command.libraryId)
          ));
          if (outcome.status === 'notRun') {
            commit(environment, () => ({
              ...before,
              phase: before.catalog ? 'ready' : 'idle',
            }));
            return { status: 'notRun', reason: 'writeBlocked' };
          }
          const catalog = outcome.value;
          if (!currentGeneration(environment, generation)) {
            return { status: 'succeeded', snapshot: getSnapshot(environment) };
          }
          const selectedLibraryId = selectedAfterDelete(before, catalog, command.libraryId);
          const keepDetail = selectedLibraryId !== null
            && selectedLibraryId === before.selectedLibraryId
            && before.detail?.id === selectedLibraryId;
          let detail = keepDetail ? before.detail : null;
          let detailPhase: LibraryDetailPhase = selectedLibraryId ? 'loading' : 'idle';
          let detailError: AppError | null = null;
          if (selectedLibraryId && !keepDetail) {
            try {
              detail = await getSkillLibrary(environment, selectedLibraryId);
              detailPhase = 'ready';
            } catch (error) {
              detailError = toAppError(error);
              detailPhase = 'error';
            }
          } else if (keepDetail) {
            detailPhase = 'ready';
          }
          if (!currentGeneration(environment, generation)) {
            return { status: 'succeeded', snapshot: getSnapshot(environment) };
          }
          const snapshot = commit(environment, (current) => ({
            ...current,
            phase: 'ready',
            catalog,
            selectedLibraryId,
            detail,
            detailPhase,
            detailError,
            catalogError: null,
          }));
          return { status: 'succeeded', snapshot };
        }
        if (command.kind === 'create') {
          const catalog = await createSkillLibrary(environment, command.name);
          const created = catalog.libraries[catalog.libraries.length - 1]?.id ?? null;
          const snapshot = await selectFromCatalog(environment, catalog, created, generation);
          return { status: 'succeeded', snapshot };
        }
        if (command.kind === 'rename') {
          const catalog = await renameSkillLibrary(
            environment,
            command.libraryId,
            command.name,
          );
          const snapshot = await selectFromCatalog(
            environment,
            catalog,
            command.libraryId,
            generation,
          );
          return { status: 'succeeded', snapshot };
        }
        if (command.kind === 'discardAddSkills') {
          const snapshot = commit(environment, (current) => ({
            ...current,
            phase: 'ready',
            pendingAdd: null,
            retryAdd: null,
            lastAddResults: [],
            catalogError: null,
          }));
          return { status: 'succeeded', snapshot };
        }
        if (command.kind === 'retryAddPreview') {
          const retry = getSnapshot(environment).retryAdd;
          if (!retry) throw new Error('Library add retry request is missing');
          const preview = await previewAddLibrarySkills(retry.request);
          if (!currentGeneration(environment, generation)) {
            return { status: 'succeeded', snapshot: getSnapshot(environment) };
          }
          const snapshot = commit(environment, (current) => ({
            ...current,
            phase: 'ready',
            pendingAdd: { request: retry.request, preview },
            retryAdd: null,
            lastAddResults: [],
            catalogError: null,
          }));
          return { status: 'succeeded', snapshot };
        }
        if (command.kind === 'confirmAddSkills') {
          const pending = getSnapshot(environment).pendingAdd;
          if (!pending) throw new Error('Library add preview is missing');
          const response = await addSkillsToLibrary({
            request: pending.request,
            expectedToken: pending.preview.token,
            acknowledgeRedirect: command.acknowledgeRedirect,
          });
          const retryableNames = new Set(
            response.results
              .filter((result) => result.status !== 'succeeded')
              .map((result) => result.skillName),
          );
          const retryRequest: PreviewAddLibrarySkillsRequest | null = retryableNames.size > 0
            ? {
              ...pending.request,
              skills: pending.request.skills.filter((item) => retryableNames.has(item.skillName)),
            }
            : null;
          let retryPreview: LibraryAddPreview | null = null;
          let retryError: AppError | null = null;
          if (retryRequest) {
            try {
              retryPreview = await previewAddLibrarySkills(retryRequest);
            } catch (error) {
              retryError = toAppError(error);
            }
          }
          const catalog = await listSkillLibraries(environment);
          if (!currentGeneration(environment, generation)) {
            return { status: 'succeeded', snapshot: getSnapshot(environment) };
          }
          const snapshot = commit(environment, (current) => ({
            ...current,
            phase: 'ready',
            catalog,
            selectedLibraryId: pending.request.libraryId,
            detail: response.library,
            pendingAdd: retryRequest && retryPreview
              ? { request: retryRequest, preview: retryPreview }
              : null,
            retryAdd: retryRequest && retryError
              ? { request: retryRequest, error: retryError }
              : null,
            lastAddResults: response.results,
            catalogError: null,
          }));
          return { status: 'succeeded', snapshot };
        }
        const fetched = command.discovery;
        const selectedPaths = command.skillPaths?.length
          ? fetched.skills
            .filter((skill) => command.skillPaths?.includes(skill.relativePath))
            .map((skill) => skill.relativePath)
          : fetched.skills.map((skill) => skill.relativePath);
        const payloads = await acquireSelectedPayloads({
          discoverySession: fetched.discoverySession,
          skillPaths: selectedPaths,
        });
        const selectedSkills = fetched.skills.filter((skill) => selectedPaths.includes(skill.relativePath));
        const request: PreviewAddLibrarySkillsRequest = {
          environment,
          libraryId: command.libraryId,
          discoverySession: fetched.discoverySession,
          skills: selectedSkills.map((skill, index) => ({
            skillName: skill.name,
            payload: payloads[index],
          })),
        };
        const preview = await previewAddLibrarySkills(request);
        if (!currentGeneration(environment, generation)) {
          return { status: 'succeeded', snapshot: getSnapshot(environment) };
        }
        const snapshot = commit(environment, (current) => ({
          ...current,
          phase: 'ready',
          selectedLibraryId: command.libraryId,
          pendingAdd: { request, preview },
          retryAdd: null,
          lastAddResults: [],
          catalogError: null,
        }));
        return { status: 'succeeded', snapshot };
      } catch (error) {
        if (!currentGeneration(environment, generation)) {
          return { status: 'succeeded', snapshot: getSnapshot(environment) };
        }
        return command.kind === 'load' || command.kind === 'select'
          ? catalogFailed(environment, error)
          : commandFailed(environment, error, before);
      }
    },
  };
}

export const libraryWorkspace = createLibraryWorkspace();
