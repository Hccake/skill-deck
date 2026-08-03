import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Plus, Search, Trash2, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { AgentIcon } from '@/components/agents/AgentIcon';
import { AgentCardGrid, SharedDirectoriesReference } from './AgentCardGrid';
import {
  AgentDefinitionFormPage,
  type AgentDefinitionFormMode,
} from './AgentDefinitionForm';
import {
  createAgentDraft,
  updateAgentDraft,
} from './agent-definition-draft';
import { filterAgentItems, type AgentListItem } from './agent-settings-presentation';
import { focusFirstAgentFieldError } from './agent-form-focus';
import { AgentDeleteDialog } from './AgentDeleteDialog';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { Input } from '@/components/ui/input';
import { environmentKey } from '@/lib/context';
import {
  useOptionalUnsavedChanges,
  useRegisterUnsavedChanges,
} from '@/lifecycle/unsaved-changes-context';
import { useAgentRegistryStore } from '@/stores/agent-registry';
import { agentDefinitionWorkflow } from '@/workflows/agent-definitions';
import { listAgents } from '@/hooks/useTauriApi';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import type {
  AgentCommandError,
  AgentDefinition,
  AgentDeleteImpact,
  AgentFieldError,
  AgentSource,
  ContextRef,
  CustomAgentDefinition,
  CustomScopeDefinition,
  InvalidCustomAgentRecord,
  ResolvedAgent,
} from '@/bindings';

interface AgentSettingsPageProps {
  context: ContextRef;
  view?: string | null;
  agentId?: string | null;
  onNavigate?: (view: 'list' | 'new' | 'edit', agentId?: string) => void;
}

type AgentSourceFilter = 'all' | AgentSource;
const agentSourceFilters: AgentSourceFilter[] = ['all', 'builtin', 'custom'];

function asAgentCommandError(error: unknown): AgentCommandError | null {
  if (!error || typeof error !== 'object' || !('kind' in error)) return null;
  const candidate = error as AgentCommandError;
  return candidate.kind === 'application'
    || candidate.kind === 'invalidDraft'
    || candidate.kind === 'staleRegistryRevision'
    ? candidate
    : null;
}

function invalidRecordLabel(raw: unknown, index: number): string {
  if (raw && typeof raw === 'object' && 'id' in raw && typeof raw.id === 'string') {
    return raw.id;
  }
  return `#${index + 1}`;
}

function customDisplayDefinition(definition: CustomAgentDefinition): AgentDefinition {
  const scope = (value: CustomScopeDefinition) => ({
    enabled: value.enabled,
    readsShared: value.enabled && value.location !== 'private',
    privatePath: value.enabled && value.location !== 'shared'
      ? value.privatePath?.kind === 'based'
        ? value.privatePath.base === 'configHome'
          ? { kind: 'configHome' as const, relativePath: value.privatePath.relativePath }
          : value.privatePath.base === 'project'
            ? { kind: 'project' as const, relativePath: value.privatePath.relativePath }
            : { kind: 'home' as const, relativePath: value.privatePath.relativePath }
        : value.privatePath
      : null,
  });
  return {
    id: definition.id,
    displayName: definition.displayName,
    source: 'custom',
    aliases: [],
    global: scope(definition.global),
    project: scope(definition.project),
    detection: {
      kind: 'anyPathExists',
      paths: definition.detectionPaths.map((path) => path.kind === 'based'
        ? path.base === 'configHome'
          ? { kind: 'configHome' as const, relativePath: path.relativePath }
          : path.base === 'project'
            ? { kind: 'project' as const, relativePath: path.relativePath }
            : { kind: 'home' as const, relativePath: path.relativePath }
        : path),
    },
    legacyPaths: [],
    adapter: 'standard',
  };
}

export function AgentSettingsPage({
  context,
  view,
  agentId,
  onNavigate,
}: AgentSettingsPageProps) {
  const { t } = useTranslation();
  const unsavedChanges = useOptionalUnsavedChanges();
  const key = environmentKey(context.environment);
  const snapshot = useAgentRegistryStore((state) => state.settingsByEnvironment[key]);
  const loadSettings = useAgentRegistryStore((state) => state.loadSettings);
  const validateDraft = useAgentRegistryStore((state) => state.validateDraft);
  const loadDeleteImpact = useAgentRegistryStore((state) => state.loadDeleteImpact);
  const businessWriteBlocked = useBusinessWriteBlocked();

  const [query, setQuery] = useState('');
  const [source, setSource] = useState<AgentSourceFilter>('all');
  const [draft, setDraft] = useState<CustomAgentDefinition | null>(null);
  const [initialDraftJson, setInitialDraftJson] = useState<string | null>(null);
  const [originalId, setOriginalId] = useState<string | null>(null);
  const [formMode, setFormMode] = useState<AgentDefinitionFormMode>('create');
  const [discardConfirmationOpen, setDiscardConfirmationOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [fieldErrors, setFieldErrors] = useState<AgentFieldError[]>([]);
  const [validationAttempted, setValidationAttempted] = useState(false);
  const [formInteracted, setFormInteracted] = useState(false);
  const [formSession, setFormSession] = useState(0);
  const [staleRevision, setStaleRevision] = useState(false);
  const [staleDeleted, setStaleDeleted] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<CustomAgentDefinition | null>(null);
  const [deleteImpact, setDeleteImpact] = useState<AgentDeleteImpact | null>(null);
  const [deletePreviewState, setDeletePreviewState] = useState<'loading' | 'ready' | 'error'>('loading');
  const [deletePreviewRevision, setDeletePreviewRevision] = useState<string | null>(null);
  const [deleteConfirmation, setDeleteConfirmation] = useState('');
  const [deleting, setDeleting] = useState(false);
  const [deleteExecutionError, setDeleteExecutionError] = useState(false);
  const [deleteStale, setDeleteStale] = useState(false);
  const [pendingSecondaryAction, setPendingSecondaryAction] = useState<string | null>(null);
  const [invalidRecord, setInvalidRecord] = useState<InvalidCustomAgentRecord | null>(null);
  const [deletingInvalid, setDeletingInvalid] = useState(false);
  const [runtimeAgents, setRuntimeAgents] = useState<Partial<Record<string, ResolvedAgent>>>({});
  const [runtimeState, setRuntimeState] = useState<'loading' | 'ready' | 'unavailable'>('loading');
  const [runtimeRetry, setRuntimeRetry] = useState(0);

  const validationRequestId = useRef(0);
  const runtimeRequestId = useRef(0);
  const handledRouteDraftKey = useRef<string | null>(null);
  const staleReloadRevision = useRef<string | null>(null);
  const deletePreviewRequestId = useRef(0);
  const detachedDraftFields = useRef<Set<string>>(new Set());
  const searchInputRef = useRef<HTMLInputElement>(null);
  const data = snapshot?.data;
  const registryRevision = data?.registryRevision;
  const readOnly = (data?.customStorageIssue?.readOnly ?? false) || businessWriteBlocked;
  const runtimeContext = useMemo<ContextRef>(() => ({
    environment: context.environment,
    scope: { scope: 'global' },
  }), [context.environment]);
  const dirty = draft !== null && initialDraftJson !== JSON.stringify(draft);
  const draftOpen = draft !== null;
  const routeDraftKey = view === 'new'
    ? 'new'
    : view === 'edit' && agentId
      ? `edit:${agentId}`
      : null;

  const resetDraftFeedback = useCallback(() => {
    setFieldErrors([]);
    setValidationAttempted(false);
    setFormInteracted(false);
    setStaleRevision(false);
    setStaleDeleted(false);
  }, []);

  const openDraftSession = useCallback((
    nextDraft: CustomAgentDefinition,
    mode: AgentDefinitionFormMode,
    nextOriginalId: string | null,
  ) => {
    staleReloadRevision.current = null;
    detachedDraftFields.current.clear();
    setDraft(nextDraft);
    setFormSession((current) => current + 1);
    setInitialDraftJson(JSON.stringify(nextDraft));
    setOriginalId(nextOriginalId);
    setFormMode(mode);
    resetDraftFeedback();
  }, [resetDraftFeedback]);

  const clearDraftSession = useCallback(() => {
    staleReloadRevision.current = null;
    detachedDraftFields.current.clear();
    handledRouteDraftKey.current = null;
    setDraft(null);
    setInitialDraftJson(null);
    setOriginalId(null);
    resetDraftFeedback();
  }, [resetDraftFeedback]);

  useEffect(() => {
    if (!snapshot || snapshot.state === 'idle') void loadSettings(context);
  }, [context, loadSettings, snapshot]);

  useEffect(() => {
    if (!registryRevision) return;
    const requestId = ++runtimeRequestId.current;
    setRuntimeAgents({});
    setRuntimeState('loading');
    void listAgents(runtimeContext).then((runtimeSnapshot) => {
      if (requestId === runtimeRequestId.current) {
        setRuntimeAgents(runtimeSnapshot.agents);
        setRuntimeState('ready');
      }
    }).catch(() => {
      if (requestId === runtimeRequestId.current) {
        setRuntimeAgents({});
        setRuntimeState('unavailable');
      }
    });
    return () => {
      if (requestId === runtimeRequestId.current) runtimeRequestId.current += 1;
    };
  }, [registryRevision, runtimeContext, runtimeRetry]);

  useEffect(() => {
    if (!routeDraftKey) {
      if (!draft) {
        handledRouteDraftKey.current = null;
        return;
      }
      if (!onNavigate || dirty) return;
      clearDraftSession();
      return;
    }
    if (handledRouteDraftKey.current === routeDraftKey || dirty) return;
    if (view === 'new') {
      handledRouteDraftKey.current = routeDraftKey;
      const nextDraft = createAgentDraft();
      setSource('custom');
      setQuery('');
      openDraftSession(nextDraft, 'create', null);
      return;
    }
    if (view === 'edit' && agentId && snapshot?.data) {
      const definition = snapshot.data.activeCustom.find((item) => item.definition.id === agentId)?.definition;
      if (definition) {
        handledRouteDraftKey.current = routeDraftKey;
        const nextDraft = structuredClone(definition);
        openDraftSession(nextDraft, 'edit', definition.id);
      } else {
        clearDraftSession();
        handledRouteDraftKey.current = routeDraftKey;
        onNavigate?.('list');
      }
    }
  }, [
    agentId,
    dirty,
    draft,
    clearDraftSession,
    onNavigate,
    openDraftSession,
    routeDraftKey,
    snapshot?.data,
    view,
  ]);

  useEffect(() => {
    if (!draftOpen) return;
    document.getElementById('agent-name')?.focus();
  }, [draftOpen, formSession]);

  useEffect(() => {
    if (!draft || readOnly || saving || staleRevision || staleDeleted) return;
    const requestId = ++validationRequestId.current;
    const timer = window.setTimeout(() => {
      void validateDraft(runtimeContext, draft, 'background')
        .then(() => {
          if (requestId !== validationRequestId.current) return;
          setFieldErrors([]);
        })
        .catch((error) => {
          if (requestId !== validationRequestId.current) return;
          const commandError = asAgentCommandError(error);
          if (commandError?.kind === 'invalidDraft') {
            setFieldErrors(commandError.errors);
          } else if (commandError?.kind === 'staleRegistryRevision') {
            setStaleRevision(true);
          }
        });
    }, 300);
    return () => window.clearTimeout(timer);
  }, [draft, readOnly, runtimeContext, saving, staleDeleted, staleRevision, validateDraft]);

  useEffect(() => {
    const previousRevision = staleReloadRevision.current;
    if (!previousRevision || !registryRevision || registryRevision === previousRevision) return;
    if (originalId) {
      const latest = data?.activeCustom.find((item) => item.definition.id === originalId)?.definition;
      if (!latest) {
        staleReloadRevision.current = null;
        setFieldErrors([]);
        setValidationAttempted(false);
        setFormInteracted(false);
        setStaleRevision(false);
        setStaleDeleted(true);
        return;
      }
      const nextDraft = structuredClone(latest);
      detachedDraftFields.current.clear();
      setDraft(nextDraft);
      setFormSession((current) => current + 1);
      setInitialDraftJson(JSON.stringify(nextDraft));
    }
    staleReloadRevision.current = null;
    setFieldErrors([]);
    setValidationAttempted(false);
    setFormInteracted(false);
    setStaleRevision(false);
    setStaleDeleted(false);
  }, [data?.activeCustom, originalId, registryRevision]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLocaleLowerCase() === 'f' && !draft) {
        event.preventDefault();
        searchInputRef.current?.focus();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [draft]);

  const builtinItems = useMemo<AgentListItem[]>(() => (data?.activeBuiltin ?? []).map((definition) => ({
    definition,
    runtime: runtimeAgents[definition.id],
  })), [data?.activeBuiltin, runtimeAgents]);
  const customItems = useMemo<AgentListItem[]>(() => (data?.activeCustom ?? []).map(({ definition }) => ({
    definition: customDisplayDefinition(definition),
    customDefinition: definition,
    runtime: runtimeAgents[definition.id],
  })), [data?.activeCustom, runtimeAgents]);
  const visibleSourceItems = useMemo(
    () => filterAgentItems(
      source === 'all'
        ? [...builtinItems, ...customItems]
        : source === 'builtin' ? builtinItems : customItems,
      query,
    ),
    [builtinItems, customItems, query, source],
  );
  const totalAgentCount = builtinItems.length + customItems.length;
  const detectedAgentCount = runtimeState === 'ready'
    ? [...builtinItems, ...customItems].filter((item) => item.runtime?.detection === 'detected').length
    : null;
  const resolvedGlobalSharedPath = Object.values(runtimeAgents)
    .find((agent) => agent?.global.sharedPath)?.global.sharedPath ?? null;
  const edit = (definition: CustomAgentDefinition) => {
    if (readOnly) return;
    const nextDraft = structuredClone(definition);
    handledRouteDraftKey.current = `edit:${definition.id}`;
    openDraftSession(nextDraft, 'edit', definition.id);
    onNavigate?.('edit', definition.id);
  };

  const startNew = () => {
    if (readOnly) return;
    const nextDraft = createAgentDraft();
    handledRouteDraftKey.current = 'new';
    setSource('custom');
    setQuery('');
    openDraftSession(nextDraft, 'create', null);
    onNavigate?.('new');
  };

  const discardDraft = async () => {
    clearDraftSession();
  };
  useRegisterUnsavedChanges({ dirty, discard: discardDraft });

  const closeDraft = async () => {
    try {
      await discardDraft();
      onNavigate?.('list');
    } catch {
      // The draft stays open when the configuration completion callback fails.
    }
  };

  const requestCloseDraft = () => {
    if (dirty && unsavedChanges) {
      void unsavedChanges.guard(() => onNavigate?.('list'));
      return;
    }
    if (dirty) {
      setDiscardConfirmationOpen(true);
      return;
    }
    void closeDraft();
  };

  const save = async () => {
    if (!draft || !data || readOnly) return;
    setValidationAttempted(true);
    setSaving(true);
    validationRequestId.current += 1;
    try {
      const currentValidation = await validateDraft(runtimeContext, draft, 'submit');
      if (!currentValidation) return;
      setFieldErrors([]);
      const result = await agentDefinitionWorkflow.save(
        context,
        draft,
        originalId,
        data.registryRevision,
      );
      if (!result) return;
      clearDraftSession();
      onNavigate?.('list');
      toast.success(t('settings.agents.saved'));
    } catch (error) {
      const commandError = asAgentCommandError(error);
      if (commandError?.kind === 'invalidDraft') {
        setFieldErrors(commandError.errors);
        focusFirstAgentFieldError(commandError.errors);
      } else if (commandError?.kind === 'staleRegistryRevision') {
        setStaleRevision(true);
      } else {
        toast.error(t('settings.agents.saveError'));
      }
    } finally {
      setSaving(false);
    }
  };

  const loadDeletePreview = async (definition: CustomAgentDefinition, revision: string) => {
    const requestId = ++deletePreviewRequestId.current;
    const action = `delete-preview:${definition.id}`;
    setDeleteImpact(null);
    setDeletePreviewState('loading');
    setDeletePreviewRevision(revision);
    setDeleteExecutionError(false);
    setPendingSecondaryAction(action);
    try {
      const impact = await loadDeleteImpact(context, definition.id, revision);
      if (requestId === deletePreviewRequestId.current && impact) {
        setDeleteImpact(impact);
        setDeletePreviewState('ready');
      } else if (requestId === deletePreviewRequestId.current) {
        setDeletePreviewState('error');
      }
    } catch {
      if (requestId === deletePreviewRequestId.current) setDeletePreviewState('error');
    } finally {
      setPendingSecondaryAction((current) => current === action ? null : current);
    }
  };

  const previewDelete = (definition: CustomAgentDefinition) => {
    if (!data || readOnly || pendingSecondaryAction) return;
    setDeleteTarget(definition);
    setDeleteConfirmation('');
    setDeleteExecutionError(false);
    setDeleteStale(false);
    void loadDeletePreview(definition, data.registryRevision);
  };

  const confirmDelete = async () => {
    if (!deleteImpact || readOnly) return;
    setDeleteExecutionError(false);
    setDeleting(true);
    try {
      const result = await agentDefinitionWorkflow.delete(
        context,
        deleteImpact.agentId,
        deleteImpact.registryRevision,
      );
      if (!result) return;
      setDeleteTarget(null);
      setDeleteImpact(null);
      setDeleteConfirmation('');
      toast.success(t('settings.agents.deleted'));
      for (const warning of result.warnings) {
        toast.warning(t(`settings.agents.warnings.${warning.code}`));
      }
    } catch (error) {
      const commandError = asAgentCommandError(error);
      if (commandError?.kind === 'staleRegistryRevision' && deleteTarget) {
        setDeleting(false);
        setDeleteConfirmation('');
        setDeleteStale(true);
        await loadDeletePreview(deleteTarget, commandError.actual);
      } else {
        setDeleteExecutionError(true);
      }
    } finally {
      setDeleting(false);
    }
  };

  const confirmInvalidDelete = async () => {
    if (!invalidRecord || !data || deletingInvalid || readOnly) return;
    setDeletingInvalid(true);
    try {
      const result = await agentDefinitionWorkflow.deleteInvalid(
        context,
        invalidRecord.index,
        data.registryRevision,
      );
      if (!result) return;
      setInvalidRecord(null);
      toast.success(t('settings.agents.deleted'));
      for (const warning of result.warnings) {
        toast.warning(t(`settings.agents.warnings.${warning.code}`));
      }
    } catch {
      toast.error(t('settings.agents.invalidDeleteError'));
    } finally {
      setDeletingInvalid(false);
    }
  };

  const reloadStaleDraft = async () => {
    staleReloadRevision.current = registryRevision ?? null;
    await loadSettings(context);
  };

  if (snapshot?.state === 'error' && !data) {
    return (
      <div className="space-y-4">
        <Alert variant="destructive">
          <AlertDescription>{t('settings.agents.loadError')}</AlertDescription>
        </Alert>
        <Button type="button" variant="outline" onClick={() => void loadSettings(context)}>
          {t('common.retry')}
        </Button>
      </div>
    );
  }

  if (!data) {
    return (
      <div className="space-y-4">
        <div className="space-y-1">
          <h2 className="text-lg font-semibold text-foreground">{t('settings.agents.title')}</h2>
          <p className="text-sm leading-6 text-muted-foreground">{t('settings.agents.description')}</p>
        </div>
        <div role="status" className="py-10 text-center text-sm text-muted-foreground">
          {t('common.loading')}
        </div>
      </div>
    );
  }

  return (
    <div className={draft ? 'min-h-full' : 'space-y-5'}>
      {draft ? (
        <AgentDefinitionFormPage
          key={formSession}
          draft={draft}
          mode={formMode}
          originalId={originalId}
          errors={formInteracted || validationAttempted ? fieldErrors : []}
          readOnly={readOnly}
          saving={saving}
          stale={staleRevision}
          deleted={staleDeleted}
          onChange={(nextDraft) => {
            setFormInteracted(true);
            setDraft((current) => (
              current && !originalId
                ? updateAgentDraft(current, nextDraft, detachedDraftFields.current)
                : nextDraft
            ));
          }}
          onBack={requestCloseDraft}
          onSave={() => void save()}
          onReload={() => void reloadStaleDraft()}
        />
      ) : (
        <>
      <div
        data-slot="agent-settings-header"
        className="flex flex-wrap items-start justify-between gap-3"
      >
        <div className="max-w-2xl space-y-1">
          <h2 className="text-lg font-semibold text-foreground">{t('settings.agents.title')}</h2>
          <p className="text-sm leading-6 text-muted-foreground">{t('settings.agents.description')}</p>
        </div>
        <Button size="sm" disabled={readOnly} onClick={startNew}>
          <Plus className="h-3.5 w-3.5" />
          {t('settings.agents.add')}
        </Button>
      </div>

      {snapshot?.state === 'loading' ? (
        <p role="status" className="text-xs text-muted-foreground">
          {t('settings.agents.refreshing')}
        </p>
      ) : null}

      {data?.customStorageIssue ? (
        <Alert variant="destructive">
          <AlertDescription>
            {t(`settings.agents.storageIssues.${data.customStorageIssue.code}`)}
          </AlertDescription>
        </Alert>
      ) : null}

      {snapshot?.state === 'error' ? (
        <Alert variant="destructive">
          <AlertDescription className="flex flex-wrap items-center justify-between gap-3">
            <span>{t('settings.agents.refreshError')}</span>
            <Button type="button" variant="outline" size="sm" onClick={() => void loadSettings(context)}>
              {t('common.retry')}
            </Button>
          </AlertDescription>
        </Alert>
      ) : null}

      {runtimeState === 'unavailable' ? (
        <Alert className="border-warning/50">
          <AlertDescription className="flex flex-wrap items-center justify-between gap-3">
            <span>{t('settings.agents.runtimeError')}</span>
            <Button type="button" variant="outline" size="sm" onClick={() => setRuntimeRetry((value) => value + 1)}>
              {t('common.retry')}
            </Button>
          </AlertDescription>
        </Alert>
      ) : null}

      {data && (data.disabledConflicts.length > 0 || data.invalidCustomRecords.length > 0) ? (
        <section className="space-y-3">
          <h3 className="text-sm font-semibold text-warning">{t('settings.agents.needsAttentionTitle')}</h3>
          {data.disabledConflicts.map((conflict) => (
            <div key={conflict.definition.id} className="flex items-center gap-3 rounded-md border border-warning/40 p-3 text-sm">
              <AgentIcon agentId={conflict.definition.id} className="h-8 w-8" />
              <div className="min-w-0 flex-1">
                <p className="truncate font-medium">{conflict.definition.displayName}</p>
                <p className="truncate text-xs text-muted-foreground">{conflict.definition.id}</p>
                <p className="mt-1 text-xs leading-5 text-warning">
                  {t('settings.agents.builtinConflict', { name: conflict.builtin.displayName })}
                </p>
              </div>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    className="shrink-0 text-muted-foreground hover:bg-destructive/10 hover:text-destructive focus-visible:text-destructive"
                    disabled={readOnly || pendingSecondaryAction !== null}
                    aria-label={t('settings.agents.deleteNamed', { name: conflict.definition.displayName })}
                    onClick={() => void previewDelete(conflict.definition)}
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>
                  {t('settings.agents.deleteNamed', { name: conflict.definition.displayName })}
                </TooltipContent>
              </Tooltip>
            </div>
          ))}
          {data.invalidCustomRecords.map((record) => (
            <div key={record.index} className="flex items-center justify-between gap-3 rounded-md border border-destructive/30 p-3 text-sm">
              <div className="min-w-0">
                <p className="truncate font-medium" translate="no">{invalidRecordLabel(record.raw, record.index)}</p>
                <p className="mt-1 text-xs text-destructive">
                  {record.errors.map((error) => t(`settings.agents.validation.${error.code}`)).join(', ')}
                </p>
              </div>
              <Button
                variant="ghost"
                size="icon"
                disabled={readOnly || deletingInvalid}
                aria-label={t('settings.agents.reviewInvalid')}
                onClick={() => setInvalidRecord(record)}
              >
                <Trash2 className="h-4 w-4" />
              </Button>
            </div>
          ))}
        </section>
      ) : null}

      <section
        role="group"
        aria-label={t('settings.agents.summary.label')}
        className="flex flex-wrap items-center gap-x-8 gap-y-3"
      >
        <div className="flex items-baseline gap-2">
          <span
            data-slot="agent-total-count"
            className="text-xl font-semibold tabular-nums text-foreground"
          >
            {totalAgentCount}
          </span>
          <span className="text-xs text-muted-foreground">{t('settings.agents.summary.total')}</span>
        </div>
        <div className="flex items-baseline gap-2">
          <span
            data-slot="agent-detected-count"
            className="text-xl font-semibold tabular-nums text-foreground"
          >
            {detectedAgentCount ?? '—'}
          </span>
          <span className="text-xs text-muted-foreground">{t('settings.agents.summary.detected')}</span>
        </div>
      </section>

      <SharedDirectoriesReference
        resolvedGlobalPath={resolvedGlobalSharedPath}
        runtimeState={runtimeState}
      />

      <div className="space-y-3">
        <div
          role="toolbar"
          aria-label={t('settings.agents.registryToolbar')}
          className="flex flex-col gap-2 sm:flex-row sm:items-center"
        >
          <div className="flex items-center justify-between gap-2 sm:contents">
            <div
              role="group"
              aria-label={t('settings.agents.sourceFilter.label')}
              className="inline-grid grid-cols-3 rounded-md border border-border/60 bg-muted/25 p-0.5 sm:shrink-0"
            >
              {agentSourceFilters.map((filter) => {
                const filterCount = filter === 'all'
                  ? builtinItems.length + customItems.length
                  : filter === 'builtin' ? builtinItems.length : customItems.length;
                const countDescriptionId = `agent-source-${filter}-count`;
                return (
                  <button
                    key={filter}
                    type="button"
                    data-agent-source-filter={filter}
                    aria-label={t(`settings.agents.sourceFilter.${filter}`)}
                    aria-describedby={countDescriptionId}
                    aria-pressed={source === filter}
                    onClick={() => {
                      setSource(filter);
                      setQuery('');
                    }}
                    onKeyDown={(event) => {
                      if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
                      event.preventDefault();
                      const direction = event.key === 'ArrowRight' ? 1 : -1;
                      const currentIndex = agentSourceFilters.indexOf(filter);
                      const nextIndex = (
                        currentIndex + direction + agentSourceFilters.length
                      ) % agentSourceFilters.length;
                      const nextFilter = agentSourceFilters[nextIndex];
                      setSource(nextFilter);
                      setQuery('');
                      event.currentTarget.parentElement
                        ?.querySelector<HTMLButtonElement>(`[data-agent-source-filter="${nextFilter}"]`)
                        ?.focus();
                    }}
                    className={cn(
                      'flex h-7 items-center justify-center gap-1.5 rounded px-3 text-xs transition-colors',
                      source === filter
                        ? 'bg-background font-medium text-foreground shadow-sm'
                        : 'text-muted-foreground hover:text-foreground',
                    )}
                  >
                    {t(`settings.agents.sourceFilter.${filter}`)}
                    <span aria-hidden="true" className="text-muted-foreground">
                      {filterCount}
                    </span>
                    <span id={countDescriptionId} className="sr-only">
                      {t('settings.agents.sourceFilter.count', { count: filterCount })}
                    </span>
                  </button>
                );
              })}
            </div>
          </div>

          <div className="relative w-full sm:order-2 sm:ml-auto sm:max-w-sm">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              ref={searchInputRef}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Escape' && query) {
                  event.preventDefault();
                  setQuery('');
                }
              }}
              placeholder={t(`settings.agents.search.${source}`)}
              aria-label={t(`settings.agents.search.${source}`)}
              className="h-9 bg-background pl-8 pr-8 text-sm shadow-none"
            />
            {query ? (
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                aria-label={t('settings.agents.search.clear')}
                className="absolute right-1.5 top-1/2 -translate-y-1/2 text-muted-foreground"
                onClick={() => setQuery('')}
              >
                <X className="h-3.5 w-3.5" />
              </Button>
            ) : null}
          </div>
        </div>

        <AgentCardGrid
          items={visibleSourceItems}
          source={source}
          query={query}
          actionsDisabled={readOnly || pendingSecondaryAction !== null}
          runtimeState={runtimeState}
          onClearQuery={() => setQuery('')}
          onAddCustom={startNew}
          onEdit={edit}
          onDelete={(definition) => void previewDelete(definition)}
        />
      </div>
        </>
      )}

      <AlertDialog open={discardConfirmationOpen} onOpenChange={setDiscardConfirmationOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('settings.agents.dirtyNavigation.title')}</AlertDialogTitle>
            <AlertDialogDescription>{t('settings.agents.dirtyNavigation.description')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('settings.agents.dirtyNavigation.stay')}</AlertDialogCancel>
            <AlertDialogAction onClick={() => { setDiscardConfirmationOpen(false); void closeDraft(); }}>
              {t('settings.agents.dirtyNavigation.discard')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AgentDeleteDialog
        target={deleteTarget ? {
          agentId: deleteTarget.id,
          displayName: deleteTarget.displayName,
        } : null}
        impact={deleteImpact}
        previewState={deletePreviewState}
        confirmation={deleteConfirmation}
        deleting={deleting}
        writeBlocked={readOnly}
        executionError={deleteExecutionError}
        stale={deleteStale}
        onConfirmationChange={setDeleteConfirmation}
        onClose={() => {
          deletePreviewRequestId.current += 1;
          setDeleteTarget(null);
          setDeleteImpact(null);
          setDeletePreviewRevision(null);
          setDeleteConfirmation('');
          setDeleteExecutionError(false);
          setDeleteStale(false);
        }}
        onConfirm={() => void confirmDelete()}
        onRetryPreview={() => {
          if (deleteTarget && (deletePreviewRevision || data)) {
            void loadDeletePreview(
              deleteTarget,
              deletePreviewRevision ?? data?.registryRevision ?? '',
            );
          }
        }}
      />

      <AlertDialog
        open={invalidRecord !== null}
        onOpenChange={(open) => {
          if (!open && !deletingInvalid) setInvalidRecord(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('settings.agents.invalidDetailTitle')}</AlertDialogTitle>
            <AlertDialogDescription>{t('settings.agents.invalidDetailDescription')}</AlertDialogDescription>
          </AlertDialogHeader>
          {invalidRecord ? (
            <pre className="max-h-64 overflow-auto rounded-md border bg-muted/30 p-3 text-xs whitespace-pre-wrap break-all" translate="no">
              {JSON.stringify(invalidRecord.raw, null, 2)}
            </pre>
          ) : null}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deletingInvalid}>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction
              disabled={deletingInvalid || readOnly}
              onClick={(event) => {
                event.preventDefault();
                void confirmInvalidDelete();
              }}
            >
              {t('settings.agents.confirmInvalidDelete')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
