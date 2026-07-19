import { Plus, Trash2 } from 'lucide-react';
import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import type {
  AgentFieldError,
  CustomAgentDefinition,
  CustomPathBase,
  CustomPathSpec,
  CustomScopeDefinition,
  ScopeLocation,
} from '@/bindings';

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

type GlobalDirectoryLocation = Exclude<CustomPathBase, 'project'> | 'absolute';

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
  const scopeError = firstError(errors, 'scopes');
  const setLocation = (location: ScopeLocation) => {
    onChange({
      ...value,
      location,
      privatePath: location === 'shared' ? null : value.privatePath ?? defaultPrivatePath(id, agentId),
    });
  };
  const setGlobalDirectoryLocation = (location: GlobalDirectoryLocation) => {
    onChange({
      ...value,
      privatePath: location === 'absolute'
        ? { kind: 'absolute', path: privatePath.kind === 'absolute' ? privatePath.path : '' }
        : {
            kind: 'based',
            base: location,
            relativePath: privatePath.kind === 'based' ? privatePath.relativePath : '',
          },
    });
  };
  const globalDirectoryLocation: GlobalDirectoryLocation = privatePath.kind === 'absolute'
    ? 'absolute'
    : privatePath.base === 'configHome' ? 'configHome' : 'home';

  return (
    <section className="space-y-3 border-t border-border/60 py-4 first:border-0 first:pt-0">
      <div className="flex items-start justify-between gap-4">
        <Label htmlFor={`${id}-enabled`} className="min-w-0 flex-1 cursor-pointer space-y-1">
          <span className="block text-sm font-medium">{t(`settings.agents.${id}.title`)}</span>
          <span className="block text-xs font-normal leading-5 text-muted-foreground">
            {t(`settings.agents.${id}.description`)}
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
        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor={`${id}-location`}>
              {t('settings.agents.skillReading.readMethod')}
            </Label>
            <Select
              value={value.location}
              disabled={disabled}
              onValueChange={(location) => setLocation(location as ScopeLocation)}
            >
              <SelectTrigger
                id={`${id}-location`}
                aria-label={t(`settings.agents.${id}.location`)}
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {(['shared', 'private', 'both'] as const).map((location) => (
                  <SelectItem key={location} value={location}>
                    {t(`settings.agents.locations.${location}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

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
              <div className={id === 'global'
                ? 'grid gap-3 sm:grid-cols-[10rem_minmax(0,1fr)]'
                : 'space-y-1.5'}
              >
                {id === 'global' ? (
                  <div className="space-y-1.5">
                    <Label htmlFor="global-directory-location">
                      {t('settings.agents.fields.directoryLocation')}
                    </Label>
                    <Select
                      value={globalDirectoryLocation}
                      disabled={disabled}
                      onValueChange={(location) => setGlobalDirectoryLocation(location as GlobalDirectoryLocation)}
                    >
                      <SelectTrigger
                        id="global-directory-location"
                        aria-label={t('settings.agents.global.directoryLocation')}
                      >
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="home">{t('settings.agents.pathBases.homeCompact')}</SelectItem>
                        <SelectItem value="configHome">{t('settings.agents.pathBases.configHome')}</SelectItem>
                        <SelectItem value="absolute">{t('settings.agents.pathKinds.absolute')}</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                ) : null}
                <div className="space-y-1.5">
                  <Label htmlFor={`${id}-path`}>
                    {id === 'project'
                      ? t('settings.agents.project.relativePath')
                      : privatePath.kind === 'absolute'
                        ? t('settings.agents.fields.absolutePath')
                        : t('settings.agents.fields.relativePath')}
                  </Label>
                  <div className={id === 'project'
                    ? 'flex min-w-0 rounded-md shadow-xs'
                    : undefined}
                  >
                    {id === 'project' ? (
                      <span className="inline-flex shrink-0 items-center rounded-l-md border border-r-0 border-input bg-muted/50 px-3 text-sm text-muted-foreground">
                        {t('settings.agents.project.pathPrefix')}
                      </span>
                    ) : null}
                    <Input
                      id={`${id}-path`}
                      name={`${id}-agent-skill-path`}
                      autoComplete="off"
                      spellCheck={false}
                      value={privatePath.kind === 'absolute' ? privatePath.path : privatePath.relativePath}
                      disabled={disabled}
                      className={id === 'project' ? 'rounded-l-none shadow-none' : undefined}
                      aria-invalid={Boolean(privatePathError)}
                      aria-describedby={privatePathError ? `${id}-path-error` : undefined}
                      onChange={(event) => onChange({
                        ...value,
                        privatePath: id === 'project'
                          ? { kind: 'based', base: 'project', relativePath: event.target.value }
                          : privatePath.kind === 'absolute'
                            ? { ...privatePath, path: event.target.value }
                            : { ...privatePath, relativePath: event.target.value },
                      })}
                    />
                  </div>
                </div>
              </div>
              {id === 'project' ? (
                <p className="text-xs text-muted-foreground">{t('settings.agents.project.relativeHint')}</p>
              ) : null}
              {privatePath.kind === 'absolute' ? (
                <p className="text-xs text-muted-foreground">{t('settings.agents.absolutePathHint')}</p>
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
      <FieldError id={`${id}-scope-error`} error={scopeError} />
    </section>
  );
}

type DetectionDirectoryLocation = CustomPathBase | 'absolute';

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
  const collectionError = errors.find((error) => error.field === 'detectionPaths');
  const update = (index: number, path: CustomPathSpec) => {
    onChange(value.map((current, currentIndex) => currentIndex === index ? path : current));
  };

  return (
    <section className="space-y-3 border-t border-border/60 pt-5">
      <div className="space-y-1">
        <h3 className="text-sm font-semibold">{t('settings.agents.installDetection.title')}</h3>
        <p className="text-xs leading-5 text-muted-foreground">{t('settings.agents.installDetection.hint')}</p>
      </div>
      {value.map((path, index) => {
        const pathError = firstError(errors, `detectionPaths[${index}]`);
        const location: DetectionDirectoryLocation = path.kind === 'absolute' ? 'absolute' : path.base;
        return (
          <div key={index} className="space-y-1.5">
            <div className="grid gap-2 sm:grid-cols-[10rem_minmax(0,1fr)_auto]">
              <Select
                value={location}
                disabled={disabled}
                onValueChange={(nextLocation) => update(index, nextLocation === 'absolute'
                  ? { kind: 'absolute', path: path.kind === 'absolute' ? path.path : '' }
                  : {
                      kind: 'based',
                      base: nextLocation as CustomPathBase,
                      relativePath: path.kind === 'based' ? path.relativePath : '',
                    })}
              >
                <SelectTrigger aria-label={t('settings.agents.detection.directoryLocation')}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="home">{t('settings.agents.pathBases.homeCompact')}</SelectItem>
                  <SelectItem value="configHome">{t('settings.agents.pathBases.configHome')}</SelectItem>
                  <SelectItem value="project">{t('settings.agents.pathBases.project')}</SelectItem>
                  <SelectItem value="absolute">{t('settings.agents.pathKinds.absolute')}</SelectItem>
                </SelectContent>
              </Select>
              <Input
                id={`detection-path-${index}`}
                name={`agent-detection-path-${index}`}
                autoComplete="off"
                spellCheck={false}
                aria-label={t('settings.agents.detection.path')}
                value={path.kind === 'absolute' ? path.path : path.relativePath}
                disabled={disabled}
                aria-invalid={Boolean(pathError)}
                aria-describedby={pathError ? `detection-path-${index}-error` : undefined}
                placeholder={path.kind === 'absolute'
                  ? t('settings.agents.detection.absolutePlaceholder')
                  : t('settings.agents.fields.relativePath')}
                onChange={(event) => update(index, path.kind === 'absolute'
                  ? { ...path, path: event.target.value }
                  : { ...path, relativePath: event.target.value })}
              />
              <Button
                type="button"
                variant="ghost"
                size="icon"
                disabled={disabled || value.length === 1}
                aria-label={t('settings.agents.detection.remove')}
                onClick={() => onChange(value.filter((_, currentIndex) => currentIndex !== index))}
              >
                <Trash2 className="h-4 w-4" />
              </Button>
            </div>
            <FieldError id={`detection-path-${index}-error`} error={pathError} />
          </div>
        );
      })}
      <FieldError id="detection-paths-error" error={collectionError} />
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={disabled}
        onClick={() => onChange([...value, { kind: 'based', base: 'home', relativePath: '' }])}
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
            <Label htmlFor="agent-name">{t('settings.agents.fields.displayName')}</Label>
            <Input
              id="agent-name"
              name="agent-display-name"
              autoComplete="off"
              value={composingDisplayName ?? draft.displayName}
              disabled={disabled}
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
            <Label htmlFor="agent-id">{t('settings.agents.fields.id')}</Label>
            <Input
              id="agent-id"
              name="agent-id"
              autoComplete="off"
              spellCheck={false}
              value={draft.id}
              disabled={disabled}
              readOnly={Boolean(originalId) || idReadOnly}
              aria-invalid={Boolean(idError)}
              aria-describedby={idError ? 'agent-id-error' : undefined}
              onChange={(event) => onChange({ ...draft, id: event.target.value })}
            />
            <FieldError id="agent-id-error" error={idError} />
          </div>
        </div>
      </section>

      <section>
        <h3 className="pb-3 text-sm font-semibold">{t('settings.agents.skillReading.title')}</h3>
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
