import { ArrowLeft, Loader2, LockKeyhole, Plus, Trash2 } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group';
import { Switch } from '@/components/ui/switch';
import type {
  AgentFieldError,
  CustomAgentDefinition,
  CustomPathSpec,
  CustomScopeDefinition,
  ScopeLocation,
} from '@/bindings';
import { cn } from '@/lib/utils';
import { AgentPathRuleField } from './AgentPathRuleField';

function firstError(errors: AgentFieldError[], field: string): AgentFieldError | undefined {
  return errors.find((error) => error.field === field || error.field.startsWith(`${field}.`));
}

function FieldError({ error, id }: { error?: AgentFieldError; id: string }) {
  const { t } = useTranslation();
  return error ? (
    <p id={id} className="text-xs text-destructive" role="status" aria-live="polite">
      {t(`settings.agents.validation.${error.code}`)}
    </p>
  ) : null;
}

function RequiredLabel({ htmlFor, children }: { htmlFor: string; children: React.ReactNode }) {
  return (
    <Label htmlFor={htmlFor}>
      {children}
      <span aria-hidden="true" className="text-destructive">*</span>
    </Label>
  );
}

function defaultPrivatePath(scope: 'global' | 'project', agentId: string): CustomPathSpec {
  return {
    kind: 'based',
    base: scope === 'global' ? 'home' : 'project',
    relativePath: agentId ? `.${agentId}/skills` : '',
  };
}

function scopeReadsPrivate(location: ScopeLocation): boolean {
  return location === 'private' || location === 'both';
}

function ScopeEditor({
  id,
  agentId,
  value,
  errors,
  disabled,
  onChange,
}: {
  id: 'global' | 'project';
  agentId: string;
  value: CustomScopeDefinition;
  errors: AgentFieldError[];
  disabled: boolean;
  onChange: (value: CustomScopeDefinition) => void;
}) {
  const { t } = useTranslation();
  const privatePath = value.privatePath ?? defaultPrivatePath(id, agentId);
  const privatePathError = firstError(errors, `${id}.privatePath`);
  const titleId = `${id}-reading-title`;
  const readRuleId = `${id}-read-rule-label`;
  const privatePathHintId = privatePath.kind === 'absolute' ? `${id}-path-hint` : null;
  const setLocation = (location: ScopeLocation) => {
    onChange({
      ...value,
      location,
      privatePath: location === 'shared' ? null : value.privatePath ?? defaultPrivatePath(id, agentId),
    });
  };
  return (
    <section
      className="space-y-4 border-t border-border/60 py-6 first:border-0 first:pt-0"
      aria-labelledby={titleId}
    >
      <div className="flex items-center justify-between gap-5">
        <Label htmlFor={`${id}-enabled`} className="min-w-0 flex-1 cursor-pointer">
          <span id={titleId} role="heading" aria-level={4} className="block text-sm font-semibold">
            {t(`settings.agents.${id}.readTitle`)}
          </span>
        </Label>
        <Switch
          id={`${id}-enabled`}
          checked={value.enabled}
          disabled={disabled}
          aria-label={t(`settings.agents.${id}.enabled`)}
          onCheckedChange={(enabled) => onChange({ ...value, enabled })}
        />
      </div>

      {value.enabled ? (
        <div className="ml-1 space-y-5 border-l-2 border-border/70 pl-4 sm:pl-5">
          <fieldset className="space-y-2">
            <legend id={readRuleId} className="mb-2 text-sm font-medium text-foreground">
              {t('settings.agents.skillReading.readMethod')}
            </legend>
            <RadioGroup
              value={value.location}
              disabled={disabled}
              className="grid w-full max-w-lg gap-1 rounded-md bg-muted/60 p-1 sm:grid-cols-3"
              aria-labelledby={readRuleId}
              onValueChange={(location) => setLocation(location as ScopeLocation)}
            >
              {(['shared', 'private', 'both'] as const).map((location) => {
                const locationId = `${id}-location-${location}`;
                return (
                  <Label
                    key={location}
                    htmlFor={locationId}
                    className={cn(
                      'flex min-h-10 min-w-0 cursor-pointer items-center justify-center rounded-md px-2 py-1.5 text-center text-xs font-medium leading-4 text-muted-foreground transition-colors focus-within:ring-[3px] focus-within:ring-ring/50 sm:min-h-9 sm:text-sm',
                      value.location === location && 'bg-background text-foreground shadow-xs',
                    )}
                  >
                    <RadioGroupItem id={locationId} value={location} className="sr-only" />
                    <span className="min-w-0">{t(`settings.agents.locations.${location}`)}</span>
                  </Label>
                );
              })}
            </RadioGroup>
          </fieldset>

          {value.location !== 'private' ? (
            <div className="flex min-w-0 items-baseline gap-3 rounded-md bg-muted/35 px-3 py-2 text-xs">
              <span className="shrink-0 text-muted-foreground">
                {t('settings.agents.directoryKind.shared')}
              </span>
              <code className="min-w-0 truncate font-mono text-foreground" translate="no">
                {id === 'global' ? '~/.agents/skills' : '.agents/skills'}
              </code>
            </div>
          ) : null}

          {scopeReadsPrivate(value.location) ? (
            <div className="space-y-2">
              <RequiredLabel htmlFor={`${id}-path`}>
                {t('settings.agents.directoryKind.private')}
              </RequiredLabel>
              <AgentPathRuleField
                id={`${id}-path`}
                name={`${id}-agent-skill-path`}
                value={privatePath}
                allowedLocations={id === 'global'
                  ? ['home', 'configHome', 'absolute']
                  : ['project']}
                locationAriaLabel={t(`settings.agents.${id}.directoryLocation`)}
                pathAriaLabel={t('settings.agents.directoryKind.private')}
                describedBy={[
                  privatePathHintId,
                  privatePathError ? `${id}-path-error` : null,
                ].filter(Boolean).join(' ') || undefined}
                invalid={Boolean(privatePathError)}
                disabled={disabled}
                required
                onChange={(privatePath) => onChange({ ...value, privatePath })}
              />
              {privatePath.kind === 'absolute' ? (
                <p id={`${id}-path-hint`} className="text-xs text-muted-foreground">
                  {t('settings.agents.absolutePathHint')}
                </p>
              ) : null}
              <FieldError id={`${id}-path-error`} error={privatePathError} />
            </div>
          ) : null}
        </div>
      ) : (
        <p className="text-xs leading-5 text-muted-foreground">
          {t(`settings.agents.readMode.${id}Unsupported`)}
        </p>
      )}
    </section>
  );
}

function DetectionPathsEditor({
  value,
  errors,
  disabled,
  onChange,
}: {
  value: CustomPathSpec[];
  errors: AgentFieldError[];
  disabled: boolean;
  onChange: (value: CustomPathSpec[]) => void;
}) {
  const { t } = useTranslation();
  const [announcement, setAnnouncement] = useState({ sequence: 0, message: '' });
  const pendingFocusIndex = useRef<number | null>(null);
  const inputRefs = useRef<Array<HTMLInputElement | null>>([]);
  const nextRowId = useRef(value.length);
  const [rowIds, setRowIds] = useState(() => (
    value.map((_, index) => `detection-row-${index}`)
  ));
  const collectionError = errors.find((error) => error.field === 'detectionPaths');
  const update = (index: number, path: CustomPathSpec) => {
    onChange(value.map((current, currentIndex) => currentIndex === index ? path : current));
  };
  const addPath = () => {
    const nextIndex = value.length;
    setRowIds((current) => [...current, `detection-row-${nextRowId.current++}`]);
    pendingFocusIndex.current = nextIndex;
    setAnnouncement((current) => ({
      sequence: current.sequence + 1,
      message: `${t('settings.agents.detection.added')} ${nextIndex + 1}`,
    }));
    onChange([...value, { kind: 'based', base: 'home', relativePath: '' }]);
  };
  const removePath = (index: number) => {
    const nextFocusIndex = Math.min(index, value.length - 2);
    pendingFocusIndex.current = nextFocusIndex >= 0 ? nextFocusIndex : null;
    setAnnouncement((current) => ({
      sequence: current.sequence + 1,
      message: `${t('settings.agents.detection.removed')} ${index + 1}`,
    }));
    setRowIds((current) => current.filter((_, currentIndex) => currentIndex !== index));
    onChange(value.filter((_, currentIndex) => currentIndex !== index));
  };

  useEffect(() => {
    const index = pendingFocusIndex.current;
    if (index === null) return;
    pendingFocusIndex.current = null;
    inputRefs.current[index]?.focus();
  }, [value.length]);

  useEffect(() => {
    setRowIds((current) => {
      if (current.length === value.length) return current;
      if (current.length > value.length) return current.slice(0, value.length);
      const next = [...current];
      while (next.length < value.length) {
        next.push(`detection-row-${nextRowId.current++}`);
      }
      return next;
    });
  }, [value.length]);

  return (
    <section className="space-y-3 border-t border-border/60 pt-5">
      <div className="space-y-1">
        <h3 className="text-sm font-semibold">{t('settings.agents.installDetection.title')}</h3>
        <p className="text-xs leading-5 text-muted-foreground">{t('settings.agents.installDetection.hint')}</p>
      </div>
      <div className="divide-y divide-border/60">
        {value.map((path, index) => {
          const pathError = firstError(errors, `detectionPaths[${index}]`);
          const pathLabel = `${t('settings.agents.detection.pathLabel')} ${index + 1}`;
          return (
            <fieldset
              key={rowIds[index] ?? `detection-row-pending-${index}`}
              className="space-y-2 py-4 first:pt-1 last:pb-1"
            >
              <legend className="mb-2 text-xs font-medium text-muted-foreground">
                {pathLabel}
                <span aria-hidden="true" className="ml-1 text-destructive">*</span>
              </legend>
              <div className="grid grid-cols-[minmax(0,1fr)_2.5rem] items-end gap-2">
                <AgentPathRuleField
                  id={`detection-path-${index}`}
                  name={`agent-detection-path-${index}`}
                  value={path}
                  allowedLocations={['home', 'configHome', 'project', 'absolute']}
                  locationAriaLabel={`${t('settings.agents.detection.directoryLocation')} ${index + 1}`}
                  pathAriaLabel={`${t('settings.agents.detection.pathInput')} ${index + 1}`}
                  describedBy={pathError ? `detection-path-${index}-error` : undefined}
                  invalid={Boolean(pathError)}
                  disabled={disabled}
                  required
                  inputRef={(element) => { inputRefs.current[index] = element; }}
                  onChange={(nextPath) => update(index, nextPath)}
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-10 w-10"
                  disabled={disabled || value.length === 1}
                  aria-label={`${t('settings.agents.detection.remove')} ${index + 1}`}
                  onClick={() => removePath(index)}
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
              <FieldError id={`detection-path-${index}-error`} error={pathError} />
            </fieldset>
          );
        })}
      </div>
      <FieldError id="detection-paths-error" error={collectionError} />
      <p className="sr-only" role="status" aria-live="polite">
        <span key={announcement.sequence}>{announcement.message}</span>
      </p>
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={disabled}
        id="detection-path-add"
        onClick={addPath}
      >
        <Plus className="h-3.5 w-3.5" />
        {t('settings.agents.detection.add')}
      </Button>
    </section>
  );
}

interface AgentDefinitionFormProps {
  draft: CustomAgentDefinition;
  originalId: string | null;
  idReadOnly?: boolean;
  errors: AgentFieldError[];
  disabled: boolean;
  stale: boolean;
  deleted?: boolean;
  onChange: (draft: CustomAgentDefinition) => void;
  onReload: () => void;
}

export function AgentDefinitionForm({
  draft,
  originalId,
  idReadOnly = false,
  errors,
  disabled,
  stale,
  deleted = false,
  onChange,
  onReload,
}: AgentDefinitionFormProps) {
  const { t } = useTranslation();
  const idError = firstError(errors, 'id');
  const nameError = firstError(errors, 'displayName');
  const scopeError = firstError(errors, 'scopes');
  const idLocked = Boolean(originalId) || idReadOnly;
  const displayNameComposition = useRef(false);
  const [composingDisplayName, setComposingDisplayName] = useState<string | null>(null);

  return (
    <div>
      {deleted ? (
        <Alert variant="destructive" className="mb-4">
          <AlertTitle>{t('settings.agents.stale.deletedTitle')}</AlertTitle>
          <AlertDescription>{t('settings.agents.stale.deletedDescription')}</AlertDescription>
        </Alert>
      ) : stale ? (
        <Alert variant="destructive" className="mb-4">
          <AlertTitle>{t('settings.agents.stale.title')}</AlertTitle>
          <AlertDescription className="space-y-3">
            <p>{t('settings.agents.stale.description')}</p>
            <Button type="button" variant="outline" size="sm" onClick={onReload}>
              {t('settings.agents.stale.reload')}
            </Button>
          </AlertDescription>
        </Alert>
      ) : null}

      <section className="space-y-3 pb-5">
        <h3 className="text-sm font-semibold">{t('settings.agents.basicInfo.title')}</h3>
        <div className="grid gap-3 sm:grid-cols-2">
          <div className="space-y-1.5">
            <div className="flex min-h-5 items-center">
              <RequiredLabel htmlFor="agent-name">{t('settings.agents.fields.displayName')}</RequiredLabel>
            </div>
            <Input
              id="agent-name"
              name="agent-display-name"
              autoComplete="off"
              value={composingDisplayName ?? draft.displayName}
              disabled={disabled}
              required
              aria-label={t('settings.agents.fields.displayName')}
              aria-invalid={Boolean(nameError)}
              aria-describedby={nameError ? 'agent-name-error' : undefined}
              onCompositionStart={(event) => {
                displayNameComposition.current = true;
                setComposingDisplayName(event.currentTarget.value);
              }}
              onCompositionEnd={(event) => {
                const displayName = event.currentTarget.value;
                displayNameComposition.current = false;
                setComposingDisplayName(null);
                onChange({ ...draft, displayName });
              }}
              onChange={(event) => {
                if (displayNameComposition.current) {
                  setComposingDisplayName(event.target.value);
                  return;
                }
                onChange({ ...draft, displayName: event.target.value });
              }}
            />
            <FieldError id="agent-name-error" error={nameError} />
          </div>
          <div className="space-y-1.5">
            <div className="flex min-h-5 items-center justify-between gap-3">
              <RequiredLabel htmlFor="agent-id">{t('settings.agents.fields.id')}</RequiredLabel>
              <span id="agent-id-hint" className="inline-flex items-center gap-1 text-xs text-muted-foreground">
                {idLocked ? <LockKeyhole className="h-3 w-3" aria-hidden="true" /> : null}
                {t(`settings.agents.fields.idHint.${idLocked ? 'locked' : 'generated'}`)}
              </span>
            </div>
            <Input
              id="agent-id"
              name="agent-id"
              autoComplete="off"
              spellCheck={false}
              value={draft.id}
              disabled={disabled}
              readOnly={idLocked}
              required
              aria-label={t('settings.agents.fields.id')}
              aria-invalid={Boolean(idError)}
              aria-describedby={idError ? 'agent-id-hint agent-id-error' : 'agent-id-hint'}
              onChange={(event) => onChange({ ...draft, id: event.target.value })}
            />
            <FieldError id="agent-id-error" error={idError} />
          </div>
        </div>
      </section>

      <section aria-labelledby="agent-skill-reading-title" aria-describedby={scopeError ? 'agent-scopes-error' : undefined}>
        <h3 id="agent-skill-reading-title" className="pb-3 text-sm font-semibold">
          {t('settings.agents.skillReading.title')}
        </h3>
        <ScopeEditor
          id="global"
          agentId={draft.id}
          value={draft.global}
          errors={errors}
          disabled={disabled}
          onChange={(global) => onChange({ ...draft, global })}
        />
        <ScopeEditor
          id="project"
          agentId={draft.id}
          value={draft.project}
          errors={errors}
          disabled={disabled}
          onChange={(project) => onChange({ ...draft, project })}
        />
        <FieldError id="agent-scopes-error" error={scopeError} />
      </section>
      <DetectionPathsEditor
        value={draft.detectionPaths}
        errors={errors}
        disabled={disabled}
        onChange={(detectionPaths) => onChange({ ...draft, detectionPaths })}
      />
    </div>
  );
}

export type AgentDefinitionFormMode = 'create' | 'edit';

interface AgentDefinitionFormPageProps extends Omit<AgentDefinitionFormProps, 'disabled' | 'idReadOnly'> {
  mode: AgentDefinitionFormMode;
  readOnly: boolean;
  saving: boolean;
  onBack: () => void;
  onSave: () => void;
}

export function AgentDefinitionFormPage({
  draft,
  mode,
  originalId,
  errors,
  readOnly,
  saving,
  stale,
  deleted = false,
  onChange,
  onBack,
  onSave,
  onReload,
}: AgentDefinitionFormPageProps) {
  const { t } = useTranslation();
  const disabled = readOnly || saving || deleted;

  return (
    <form
      className="mx-auto flex min-h-full w-full max-w-3xl flex-col"
      aria-labelledby="agent-form-title"
      aria-busy={saving}
      noValidate
      onSubmit={(event) => {
        event.preventDefault();
        onSave();
      }}
    >
      <header className="mb-6 flex items-center gap-3 border-b border-border/60 pb-5">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-10 w-10 shrink-0 text-muted-foreground"
          disabled={saving}
          aria-label={t('settings.agents.backToList')}
          onClick={onBack}
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <div className="min-w-0">
          <h2 id="agent-form-title" className="text-xl font-semibold text-foreground">
            {t(`settings.agents.form.title.${mode}`)}
          </h2>
        </div>
      </header>

      <AgentDefinitionForm
        draft={draft}
        originalId={originalId}
        idReadOnly={mode === 'edit'}
        errors={errors}
        disabled={disabled}
        stale={stale}
        deleted={deleted}
        onChange={onChange}
        onReload={onReload}
      />

      <footer className="sticky bottom-0 z-10 mt-6 flex flex-wrap justify-end gap-2 border-t border-border/60 bg-background/95 py-3 pb-[max(0.75rem,env(safe-area-inset-bottom))] backdrop-blur supports-[backdrop-filter]:bg-background/85">
        <Button type="button" variant="outline" disabled={saving} onClick={onBack}>
          {t('common.cancel')}
        </Button>
        <Button type="submit" disabled={readOnly || saving || stale || deleted}>
          {saving ? <Loader2 className="h-3.5 w-3.5 animate-spin motion-reduce:animate-none" /> : null}
          {t(`settings.agents.form.action.${mode}`)}
        </Button>
      </footer>
    </form>
  );
}
