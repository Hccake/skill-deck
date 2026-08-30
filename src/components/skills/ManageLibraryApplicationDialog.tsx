import { useEffect, useMemo, useRef, useState } from 'react';
import { ArrowDown, ArrowUp, CircleHelp, Info, Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type {
  AgentInstallOptionId,
  AgentSelectionSnapshot,
  AppError,
  LibraryAgentOptions,
  LibraryApplicationSummary,
  LibraryId,
  LibraryWorkspaceSnapshot,
  SkillLibrarySummary,
  SkillLocationRef,
} from '@/bindings';
import { formatAppError } from '@/utils/format-app-error';
import { toAppError } from '@/utils/to-app-error';
import {
  applyLibraryApplication,
  getLibraryAgentOptions,
  listSkillLibraries,
  previewLibraryApplication,
  retryLibraryApplication,
} from '@/hooks/useTauriApi';
import { AgentSelectionView } from '@/components/agents/selection/AgentSelectionView';
import { useAgentSelectionPresentation } from '@/components/agents/selection/useAgentSelectionPresentation';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Label } from '@/components/ui/label';
import { Skeleton } from '@/components/ui/skeleton';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import {
  createAgentSelectionSession,
  toggleInstallOption,
  toggleSelectionGroup,
  type AgentSelectionSession,
} from '@/lib/agent-selection-session';
import { LibraryIdentity } from './LibraryIdentity';

interface ManageLibraryApplicationDialogProps {
  open: boolean;
  context: SkillLocationRef | null;
  projectName?: string;
  application: LibraryApplicationSummary | null;
  onOpenChange: (open: boolean) => void;
  onApplied: () => Promise<void>;
}

type LoadState = 'idle' | 'loading' | 'ready' | 'error';
type Operation = 'saving' | 'resuming' | null;
type SaveFailure =
  | { kind: 'appError'; error: AppError }
  | { kind: 'execution' };

export function ManageLibraryApplicationDialog({
  open,
  context,
  projectName,
  application,
  onOpenChange,
  onApplied,
}: ManageLibraryApplicationDialogProps) {
  const { t } = useTranslation();
  const sharedAgentPresentation = useAgentSelectionPresentation('libraryApplication');
  const agentPresentation = {
    ...sharedAgentPresentation,
    selectable: {
      title: t('libraries.linkableAgents'),
      help: t('libraries.linkableAgentsHelp'),
    },
  };
  const [catalog, setCatalog] = useState<LibraryWorkspaceSnapshot | null>(null);
  const [agentOptions, setAgentOptions] = useState<LibraryAgentOptions | null>(null);
  const [agentSession, setAgentSession] = useState<AgentSelectionSession | null>(null);
  const [selected, setSelected] = useState<LibraryId[]>([]);
  const [loadState, setLoadState] = useState<LoadState>('idle');
  const [loadAttempt, setLoadAttempt] = useState(0);
  const [operation, setOperation] = useState<Operation>(null);
  const [saveFailure, setSaveFailure] = useState<SaveFailure | null>(null);
  const [incomplete, setIncomplete] = useState(false);
  const loadRequestId = useRef(0);
  const busy = operation !== null;
  const title = context?.scope.scope === 'global'
    ? t('libraries.manageGlobal')
    : projectName
      ? t('libraries.manageProject', { name: projectName })
      : t('libraries.manageProjectFallback');

  useEffect(() => {
    if (!open || !context || !application) return;
    setSelected(application.orderedLibraries.map((library) => library.id));
    setSaveFailure(null);
    setIncomplete(false);
    setCatalog(null);
    setAgentOptions(null);
    setAgentSession(null);
    if (application.pending) {
      setLoadState('ready');
      return;
    }
    setLoadState('loading');
    const requestId = ++loadRequestId.current;
    void Promise.all([
      listSkillLibraries(context.environment),
      getLibraryAgentOptions(context),
    ]).then(([nextCatalog, nextAgentOptions]) => {
      if (requestId !== loadRequestId.current) return;
      const selection = withInitialAgents(
        nextAgentOptions.selection,
        application.selectedAgentIds,
      );
      setCatalog(nextCatalog);
      setAgentOptions({ ...nextAgentOptions, selection });
      setAgentSession(createAgentSelectionSession(selection));
      setLoadState('ready');
    }).catch(() => {
      if (requestId === loadRequestId.current) setLoadState('error');
    });
    return () => { loadRequestId.current += 1; };
  }, [application, context, loadAttempt, open]);

  const selection = agentOptions?.selection ?? null;
  const selectedAgentIds = useMemo(() => (
    selection && agentSession
      ? selectedAgentsFromOptions(selection, agentSession.selectedOptionIds)
      : []
  ), [agentSession, selection]);
  const draft = useMemo(() => (
    context && loadState === 'ready' && agentSession
      ? { context, orderedLibraryIds: selected, selectedAgentIds }
      : null
  ), [agentSession, context, loadState, selected, selectedAgentIds]);
  const dirty = application && draft
    ? !sameArray(draft.orderedLibraryIds, application.orderedLibraries.map((library) => library.id))
      || !sameSet(draft.selectedAgentIds, application.selectedAgentIds)
    : false;
  const selectedLibraries = selected.flatMap((id) => {
    const library = catalog?.libraries.find((candidate) => candidate.id === id)
      ?? application?.orderedLibraries.find((candidate) => candidate.id === id);
    return library ? [library] : [];
  });
  const availableLibraries = catalog?.libraries.filter((library) => !selected.includes(library.id)) ?? [];

  const move = (libraryId: LibraryId, offset: number) => {
    setSaveFailure(null);
    setSelected((current) => {
      const from = current.indexOf(libraryId);
      const to = from + offset;
      if (from < 0 || to < 0 || to >= current.length) return current;
      const next = [...current];
      next.splice(to, 0, next.splice(from, 1)[0]);
      return next;
    });
  };
  const removeLibrary = (libraryId: LibraryId) => {
    setSaveFailure(null);
    setSelected((current) => {
      const next = current.filter((id) => id !== libraryId);
      if (next.length === 0) {
        setAgentSession((session) => session ? { ...session, selectedOptionIds: [] } : session);
      }
      return next;
    });
  };
  const requestClose = () => {
    if (busy) return;
    onOpenChange(false);
  };
  const save = async () => {
    if (!draft || !dirty) return;
    setOperation('saving');
    setSaveFailure(null);
    try {
      const preview = await previewLibraryApplication(draft);
      const response = await applyLibraryApplication({ draft, expectedToken: preview.token });
      await onApplied();
      const completed = response.units.every((unit) => (
        unit.status === 'succeeded' || unit.status === 'skipped'
      ));
      if (response.application.pending) {
        setIncomplete(true);
      } else if (!completed) {
        setSaveFailure({ kind: 'execution' });
      } else {
        onOpenChange(false);
      }
    } catch (error) {
      setSaveFailure({ kind: 'appError', error: toAppError(error) });
    } finally {
      setOperation(null);
    }
  };
  const resume = async () => {
    if (!context) return;
    setOperation('resuming');
    setSaveFailure(null);
    try {
      const response = await retryLibraryApplication(context);
      await onApplied();
      const completed = response.units.every((unit) => (
        unit.status === 'succeeded' || unit.status === 'skipped'
      ));
      if (response.application.pending) {
        setIncomplete(true);
      } else if (!completed) {
        setSaveFailure({ kind: 'execution' });
      } else {
        onOpenChange(false);
      }
    } catch (error) {
      setSaveFailure({ kind: 'appError', error: toAppError(error) });
    } finally {
      setOperation(null);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(nextOpen) => !nextOpen && requestClose()}>
      <DialogContent
        className="grid h-[min(42rem,calc(100dvh-2rem))] w-[calc(100vw-2rem)] min-w-0 grid-rows-[auto_minmax(0,1fr)_auto] gap-0 overflow-hidden p-0 sm:max-w-5xl"
        dismissible={!busy}
        closeLabel={t('common.close')}
        aria-busy={loadState === 'loading' || busy}
      >
        <DialogHeader className="border-b px-6 pt-6 pb-4">
          <DialogTitle className="min-w-0 truncate" title={title}>{title}</DialogTitle>
          <DialogDescription className="sr-only">{t('libraries.manageDescription')}</DialogDescription>
        </DialogHeader>

        <div className="grid min-h-0 grid-rows-[auto_minmax(0,1fr)] overflow-hidden">
          {saveFailure ? (
            <LibraryApplicationSaveFailure
              failure={saveFailure}
              selection={selection}
              onCancelAgents={(agentIds) => {
                if (!selection || !agentSession) return;
                const conflictingAgentIds = new Set(agentIds);
                const nextSession = selection.installOptions.reduce(
                  (session, option) => option.agentIds.some((id) => conflictingAgentIds.has(id))
                    ? toggleInstallOption(session, selection, option.id, false)
                    : session,
                  agentSession,
                );
                setAgentSession(nextSession);
                setSaveFailure(null);
              }}
            />
          ) : null}
          {application?.pending || incomplete ? (
            <PendingApplication />
          ) : loadState === 'error' ? (
            <LoadError onRetry={() => setLoadAttempt((attempt) => attempt + 1)} />
          ) : loadState !== 'ready' || !agentOptions || !selection || !agentSession ? (
            <DialogLoading />
          ) : (
            <LibraryApplicationEditor
              libraries={selectedLibraries}
              availableLibraries={availableLibraries}
              selectedAgentCount={selectedAgentIds.length}
              selection={selection}
              agentSession={agentSession}
              agentOptions={agentOptions}
              agentPresentation={agentPresentation}
              agentsDisabled={selected.length === 0}
              onRemoveLibrary={removeLibrary}
              onAddLibrary={(id) => {
                setSaveFailure(null);
                setSelected((current) => [...current, id]);
              }}
              onMoveLibrary={move}
              onAgentSessionChange={(session) => {
                setSaveFailure(null);
                setAgentSession(session);
              }}
            />
          )}
        </div>

        <DialogFooter className="border-t px-6 py-4">
          <Button
            type="button"
            variant="outline"
            onClick={requestClose}
            disabled={busy}
          >
            {t('common.cancel')}
          </Button>
          <Button
            type="button"
            onClick={() => void (application?.pending || incomplete ? resume() : save())}
            disabled={busy || (!application?.pending && !incomplete && !dirty)}
          >
            {operation ? <Loader2 className="size-3.5 animate-spin" aria-hidden="true" /> : null}
            {application?.pending || incomplete
              ? t(operation === 'resuming' ? 'libraries.resuming' : 'libraries.continue')
              : t(operation === 'saving' ? 'libraries.saving' : 'libraries.save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function LibraryApplicationSaveFailure({
  failure,
  selection,
  onCancelAgents,
}: {
  failure: SaveFailure;
  selection: AgentSelectionSnapshot | null;
  onCancelAgents: (agentIds: string[]) => void;
}) {
  const { t } = useTranslation();
  const conflict = failure.kind === 'appError'
    && failure.error.kind === 'skillPlacementTargetConflict'
    ? failure.error
    : null;
  const conflictingAgentIds = conflict?.data.agentIds ?? [];
  const conflictingAgentNames = conflictingAgentIds.map((id) => (
    selection?.agents.find((agent) => agent.id === id)?.displayName ?? id
  ));
  const message = conflict
    ? conflictingAgentNames.length > 0
      ? t('libraries.targetConflictAgent', {
        skill: conflict.data.skillName,
        agents: conflictingAgentNames.join(', '),
      })
      : t('mutation.result.errors.skillPlacementTargetConflict', {
        skillName: conflict.data.skillName,
        targetPath: conflict.data.targetPath,
        targetKind: t(`mutation.result.targetKinds.${conflict.data.targetKind}`),
      })
    : failure.kind === 'appError'
      ? formatAppError(failure.error, t)
      : t('libraries.saveError');

  return (
    <Alert variant="destructive" className="mx-6 mt-4 w-auto">
      <AlertDescription className="gap-2">
        <p>{message}</p>
        {conflictingAgentIds.length > 0 ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => onCancelAgents(conflictingAgentIds)}
          >
            {t('libraries.cancelConflictingAgents', {
              agents: conflictingAgentNames.join(', '),
            })}
          </Button>
        ) : null}
      </AlertDescription>
    </Alert>
  );
}

function LibraryApplicationEditor({
  libraries,
  availableLibraries,
  selectedAgentCount,
  selection,
  agentSession,
  agentOptions,
  agentPresentation,
  agentsDisabled,
  onRemoveLibrary,
  onAddLibrary,
  onMoveLibrary,
  onAgentSessionChange,
}: {
  libraries: SkillLibrarySummary[];
  availableLibraries: SkillLibrarySummary[];
  selectedAgentCount: number;
  selection: AgentSelectionSnapshot;
  agentSession: AgentSelectionSession;
  agentOptions: LibraryAgentOptions;
  agentPresentation: ReturnType<typeof useAgentSelectionPresentation>;
  agentsDisabled: boolean;
  onRemoveLibrary: (id: LibraryId) => void;
  onAddLibrary: (id: LibraryId) => void;
  onMoveLibrary: (id: LibraryId, offset: number) => void;
  onAgentSessionChange: (session: AgentSelectionSession) => void;
}) {
  const { t } = useTranslation();
  const setOption = (optionId: AgentInstallOptionId, selected: boolean) => {
    onAgentSessionChange(toggleInstallOption(agentSession, selection, optionId, selected));
  };
  return (
    <div
      data-testid="library-application-dialog-body"
      className="grid min-h-0 min-w-0 grid-rows-[minmax(10rem,0.85fr)_minmax(12rem,1.15fr)] gap-4 px-6 py-4 md:grid-cols-[minmax(16rem,0.8fr)_minmax(24rem,1.2fr)] md:grid-rows-1 md:gap-8"
    >
      <section className="grid min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)] border-b pb-4 md:border-r md:border-b-0 md:pb-0 md:pr-8" aria-labelledby="applied-libraries-title">
        <div className="flex items-center justify-between gap-3 pb-3">
          <span className="flex items-center gap-1">
            <h2 id="applied-libraries-title" className="text-sm font-semibold">{t('libraries.appliedSection')}</h2>
            <Tooltip>
              <TooltipTrigger asChild>
                <button type="button" className="inline-flex size-6 items-center justify-center rounded-sm text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" aria-label={t('libraries.priorityHelp')}>
                  <CircleHelp className="size-3.5" aria-hidden="true" />
                </button>
              </TooltipTrigger>
              <TooltipContent className="max-w-64 text-xs">{t('libraries.priorityHelp')}</TooltipContent>
            </Tooltip>
          </span>
          {libraries.length > 0 ? (
            <span className="text-xs text-muted-foreground">{t('libraries.selectedCount', { count: libraries.length })}</span>
          ) : null}
        </div>
        <div data-testid="library-selection-scroll" className="min-h-0 space-y-4 overflow-y-auto overscroll-contain pr-1 [scrollbar-gutter:stable]">
          {libraries.length > 0 ? (
            <div className="space-y-1.5" aria-label={t('libraries.priorityOrder')}>
              {libraries.map((library, index) => (
                <SelectedLibraryRow
                  key={library.id}
                  library={library}
                  index={index}
                  count={libraries.length}
                  onRemove={() => onRemoveLibrary(library.id)}
                  onMove={(offset) => onMoveLibrary(library.id, offset)}
                />
              ))}
            </div>
          ) : availableLibraries.length > 0 ? (
            <p className="py-4 text-center text-sm text-muted-foreground">{t('libraries.noneSelected')}</p>
          ) : null}
          {availableLibraries.length > 0 ? (
            <section aria-labelledby="available-libraries-title" className="space-y-2">
              <h3 id="available-libraries-title" className="px-1 text-xs font-semibold text-muted-foreground">
                {t('libraries.availableSection')}
              </h3>
              <div className="space-y-1.5">
                {availableLibraries.map((library) => (
                  <AvailableLibraryRow key={library.id} library={library} onAdd={() => onAddLibrary(library.id)} />
                ))}
              </div>
            </section>
          ) : libraries.length === 0 ? (
            <p className="py-4 text-center text-sm text-muted-foreground">{t('libraries.noLibrariesAvailable')}</p>
          ) : null}
        </div>
      </section>

      <section className="grid min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)]" aria-labelledby="library-agents-title">
        <div className="flex items-center justify-between gap-3 pb-3">
          <h2 id="library-agents-title" className="text-sm font-semibold">{t('agentSelection.copyTitle')}</h2>
          {selectedAgentCount > 0 ? (
            <span className="text-xs text-muted-foreground">{t('agentSelection.ownDirectory.selectedCount', { count: selectedAgentCount })}</span>
          ) : null}
        </div>
        <div data-testid="library-agent-selection-scroll" className="min-h-0 space-y-4 overflow-y-auto overscroll-contain pr-1 [scrollbar-gutter:stable]">
          {agentsDisabled ? (
            <p className="py-4 text-center text-sm text-muted-foreground">{t('libraries.selectLibraryBeforeAgents')}</p>
          ) : (
            <AgentSelectionView
              presentation={agentPresentation}
              snapshot={selection}
              session={agentSession}
              disabled={false}
              emptyMessage={t('libraries.noLinkableAgents')}
              onOptionChange={setOption}
              onGroupChange={(groupId, selected) => onAgentSessionChange(
                toggleSelectionGroup(agentSession, selection, groupId, selected),
              )}
              onOtherExpandedChange={(expanded) => onAgentSessionChange({ ...agentSession, otherAgentsExpanded: expanded })}
              onAdditionalExpandedChange={(expanded) => onAgentSessionChange({ ...agentSession, additionalInstallExpanded: expanded })}
              onGroupExpandedChange={(groupId, expanded) => onAgentSessionChange({
                ...agentSession,
                expandedGroupIds: expanded
                  ? [...new Set([...agentSession.expandedGroupIds, groupId])]
                  : agentSession.expandedGroupIds.filter((id) => id !== groupId),
              })}
            />
          )}
          {!agentsDisabled && agentOptions.unsupportedAgentNames.length > 0 ? (
            <div className="flex gap-2 rounded-md bg-muted/60 px-3 py-2.5 text-sm text-muted-foreground" role="status">
              <Info className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
              <span>{t('libraries.copyOnlyUnsupported', { names: agentOptions.unsupportedAgentNames.join(', ') })}</span>
            </div>
          ) : null}
          {!agentsDisabled ? agentOptions.migrations
            .filter((migration) => selectedAgentsFromOptions(selection, agentSession.selectedOptionIds).includes(migration.agentId))
            .map((migration) => (
              <p key={`${migration.agentId}:${migration.fromPath}`} className="text-xs text-muted-foreground">
                {t('libraries.agentPathMigration', { name: migration.displayName, from: migration.fromPath, to: migration.toPath })}
              </p>
            )) : null}
        </div>
      </section>
    </div>
  );
}

function SelectedLibraryRow({
  library,
  index,
  count,
  onRemove,
  onMove,
}: {
  library: SkillLibrarySummary;
  index: number;
  count: number;
  onRemove: () => void;
  onMove: (offset: number) => void;
}) {
  const { t } = useTranslation();
  return (
    <div data-testid="selected-library-row" className="grid min-h-11 grid-cols-[1rem_minmax(0,1fr)_auto] items-center gap-2 rounded-md border border-primary/15 bg-primary/[0.04] px-2.5">
      <Checkbox checked onCheckedChange={onRemove} aria-label={library.name} />
      <span className="grid min-w-0 grid-cols-[1.5rem_minmax(0,1fr)] items-center gap-2">
        <LibraryIdentity library={library} />
      </span>
      <span className="flex shrink-0 items-center gap-0.5">
        <Button type="button" variant="ghost" size="icon" className="size-7" onClick={() => onMove(-1)} disabled={index === 0} aria-label={t('libraries.moveUp', { name: library.name })}>
          <ArrowUp className="size-3.5" aria-hidden="true" />
        </Button>
        <Button type="button" variant="ghost" size="icon" className="size-7" onClick={() => onMove(1)} disabled={index === count - 1} aria-label={t('libraries.moveDown', { name: library.name })}>
          <ArrowDown className="size-3.5" aria-hidden="true" />
        </Button>
      </span>
    </div>
  );
}

function AvailableLibraryRow({ library, onAdd }: { library: SkillLibrarySummary; onAdd: () => void }) {
  return (
    <Label className="grid min-h-11 cursor-pointer grid-cols-[1rem_minmax(0,1fr)] items-center gap-2 rounded-md px-2.5 hover:bg-muted/50 has-[[data-disabled]]:cursor-not-allowed has-[[data-disabled]]:opacity-60">
      <Checkbox checked={false} disabled={library.skillCount === 0} onCheckedChange={(selected) => selected && onAdd()} aria-label={library.name} />
      <span className="grid min-w-0 grid-cols-[1.5rem_minmax(0,1fr)] items-center gap-2">
        <LibraryIdentity library={library} />
      </span>
    </Label>
  );
}

function DialogLoading() {
  const { t } = useTranslation();
  return (
    <div role="status" aria-live="polite" className="grid min-h-0 gap-8 px-6 py-5 md:grid-cols-2">
      <span className="sr-only">{t('common.loading')}</span>
      {[0, 1].map((column) => (
        <div key={column} className="space-y-3">
          <Skeleton className="h-5 w-32" />
          <Skeleton className="h-11 w-full" />
          <Skeleton className="h-11 w-full" />
          <Skeleton className="h-11 w-4/5" />
        </div>
      ))}
    </div>
  );
}

function LoadError({ onRetry }: { onRetry: () => void }) {
  const { t } = useTranslation();
  return (
    <div className="grid min-h-0 place-items-center px-6 py-5">
      <Alert className="max-w-lg">
        <AlertDescription className="flex items-center justify-between gap-4">
          <span>{t('libraries.selectionLoadError')}</span>
          <Button type="button" variant="outline" size="sm" onClick={onRetry}>{t('common.retry')}</Button>
        </AlertDescription>
      </Alert>
    </div>
  );
}

function PendingApplication() {
  const { t } = useTranslation();
  return (
    <div className="grid min-h-0 place-items-center px-6 py-5">
      <div className="max-w-lg space-y-2 text-center">
        <h2 className="font-semibold">{t('libraries.pending')}</h2>
        <p className="text-sm text-muted-foreground">{t('libraries.pendingDescription')}</p>
      </div>
    </div>
  );
}

function withInitialAgents(selection: AgentSelectionSnapshot, agentIds: string[]) {
  const selected = new Set(agentIds);
  return {
    ...selection,
    initialSelectedOptionIds: selection.installOptions
      .filter((option) => option.agentIds.length > 0 && option.agentIds.every((id) => selected.has(id)))
      .map((option) => option.id),
  };
}

function selectedAgentsFromOptions(selection: AgentSelectionSnapshot, optionIds: string[]) {
  const selected = new Set(optionIds);
  return [...new Set(selection.installOptions
    .filter((option) => selected.has(option.id))
    .flatMap((option) => option.agentIds))].sort();
}

function sameArray(left: string[], right: string[]) {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

function sameSet(left: string[], right: string[]) {
  return left.length === right.length && left.every((value) => right.includes(value));
}
