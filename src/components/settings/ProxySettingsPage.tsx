import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  CheckCircle2,
  CircleMinus,
  Info,
  LoaderCircle,
  Wifi,
  XCircle,
} from 'lucide-react';

import type {
  GitProxyScope,
  NativeGitProxySettings,
  WslGitProxySettings,
} from '@/bindings';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Skeleton } from '@/components/ui/skeleton';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import {
  getProxySettings,
  saveProxySettings,
  testProxyConnection,
  type NetworkProxySettings,
  type ProxyConnectionTestResult,
} from '@/hooks/useTauriApi';
import { useRegisterUnsavedChanges } from '@/lifecycle/unsaved-changes-context';
import { cn } from '@/lib/utils';
import { useEnvironmentStore } from '@/stores/environment';
import { toAppError } from '@/utils/to-app-error';

type ProxyMode = NetworkProxySettings['mode'];
type GitProxyBehavior = NativeGitProxySettings['behavior'];
type WslGitProxyBehavior = WslGitProxySettings['behavior'];

const HTTP_CONNECTION_TEST_TARGET = 'https://www.skills.sh/api/search?q=skill&limit=1';
const GIT_CONNECTION_TEST_TARGET = 'https://github.com/hccake/skill-deck.git';

function completeSettings(settings: NetworkProxySettings): NetworkProxySettings {
  return {
    ...settings,
    nativeGit: settings.nativeGit ?? { behavior: 'useExistingGitConfig' },
    wslGit: { ...settings.wslGit },
  };
}

function buildDraft(settings: NetworkProxySettings): NetworkProxySettings {
  const result = completeSettings(settings);
  result.customProxyUrl = result.mode === 'custom'
    ? result.customProxyUrl?.trim() || null
    : null;
  if (result.nativeGit?.behavior === 'useProxy') {
    result.nativeGit = {
      ...result.nativeGit,
      proxyUrl: result.nativeGit.proxyUrl.trim(),
    };
  }
  const wslGit: Partial<Record<string, WslGitProxySettings>> = {};
  for (const [distro, settings] of Object.entries(result.wslGit ?? {})) {
    if (!settings) continue;
    wslGit[distro] = settings.behavior === 'useProxy'
      ? { ...settings, proxyUrl: settings.proxyUrl.trim() }
      : settings;
  }
  result.wslGit = wslGit;
  return result;
}

function updateRecord<T>(
  record: Partial<Record<string, T>> | undefined,
  key: string,
  value: T,
) {
  return { ...record, [key]: value };
}

function nativeGitSettingsForBehavior(
  current: NativeGitProxySettings | undefined,
  behavior: GitProxyBehavior,
): NativeGitProxySettings {
  if (current?.behavior === behavior) return current;
  return behavior === 'useProxy'
    ? { behavior, proxyUrl: '', scope: 'githubOnly' }
    : { behavior };
}

function wslGitSettingsForBehavior(
  current: WslGitProxySettings | undefined,
  behavior: WslGitProxyBehavior,
): WslGitProxySettings {
  if (current?.behavior === behavior) return current;
  return behavior === 'useProxy'
    ? { behavior, proxyUrl: '', scope: 'githubOnly' }
    : { behavior };
}

export function ProxySettingsPage() {
  const { t } = useTranslation();
  const writeBlocked = useBusinessWriteBlocked();
  const environments = useEnvironmentStore((state) => state.environments);
  const [savedSettings, setSavedSettings] = useState<NetworkProxySettings | null>(null);
  const [draft, setDraft] = useState<NetworkProxySettings | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<ProxyConnectionTestResult | null>(null);
  const [loadAttempt, setLoadAttempt] = useState(0);
  const loadRequestId = useRef(0);
  const saveRequestId = useRef(0);
  const testRequestId = useRef(0);
  const draftRevision = useRef(0);

  useEffect(() => {
    const activeRequest = ++loadRequestId.current;
    void getProxySettings()
      .then((settings) => {
        if (loadRequestId.current !== activeRequest) return;
        const complete = completeSettings(settings);
        setSavedSettings(complete);
        setDraft(complete);
      })
      .catch(() => {
        if (loadRequestId.current === activeRequest) setError(t('settings.proxy.loadError'));
      })
      .finally(() => {
        if (loadRequestId.current === activeRequest) setLoading(false);
      });
    return () => {
      if (loadRequestId.current === activeRequest) loadRequestId.current += 1;
    };
  }, [loadAttempt, t]);

  const currentDraft = useMemo(() => draft ? buildDraft(draft) : null, [draft]);
  const changed = savedSettings !== null
    && currentDraft !== null
    && JSON.stringify(savedSettings) !== JSON.stringify(currentDraft);
  const wslDistros = useMemo(() => environments
    .flatMap((environment) => environment.environment.kind === 'wsl'
      ? [environment.environment.distro_name]
      : []), [environments]);
  const nativeEnvironmentName = environments.find(
    (environment) => environment.environment.kind === 'native',
  )?.displayName ?? t('settings.proxy.nativeEnvironment');
  const nativeGitLabel = `${nativeEnvironmentName} Git`;
  const nativeGit = draft?.nativeGit ?? { behavior: 'useExistingGitConfig' };

  const wslDistroSignature = wslDistros.join('\0');
  useEffect(() => {
    testRequestId.current += 1;
    setTesting(false);
    setTestResult(null);
  }, [wslDistroSignature]);

  const invalidateTestResult = useCallback(() => {
    testRequestId.current += 1;
    setTesting(false);
    setTestResult(null);
  }, []);

  const discardDraft = useCallback(() => {
    if (!savedSettings) return;
    draftRevision.current += 1;
    saveRequestId.current += 1;
    setSaving(false);
    setDraft(savedSettings);
    setMessage(null);
    setError(null);
    invalidateTestResult();
  }, [invalidateTestResult, savedSettings]);

  useRegisterUnsavedChanges(useMemo(() => ({
    dirty: changed,
    discard: discardDraft,
  }), [changed, discardDraft]));

  const updateDraft = (updater: (current: NetworkProxySettings) => NetworkProxySettings) => {
    draftRevision.current += 1;
    setDraft((current) => current ? updater(current) : current);
    setMessage(null);
    setError(null);
    invalidateTestResult();
  };

  const patchDraft = (patch: Partial<NetworkProxySettings>) => {
    updateDraft((current) => ({ ...current, ...patch }));
  };

  const showOperationError = (cause: unknown, fallbackKey: string) => {
    const appError = toAppError(cause);
    if (appError.kind === 'invalidProxySettings') {
      setError(t(`settings.proxy.errors.${appError.data.code}`));
      return;
    }
    setError(t(fallbackKey));
  };

  const save = async () => {
    if (!currentDraft || writeBlocked) return;
    setSaving(true);
    const activeRequest = ++saveRequestId.current;
    const savedDraftRevision = draftRevision.current;
    setMessage(null);
    setError(null);
    try {
      const saved = completeSettings(await saveProxySettings(currentDraft));
      if (saveRequestId.current !== activeRequest) return;
      setSavedSettings(saved);
      if (draftRevision.current === savedDraftRevision) {
        setDraft(saved);
        setMessage(t('settings.proxy.saved'));
      }
    } catch (cause) {
      if (saveRequestId.current !== activeRequest
        || draftRevision.current !== savedDraftRevision) return;
      showOperationError(cause, 'settings.proxy.saveError');
    } finally {
      if (saveRequestId.current === activeRequest) setSaving(false);
    }
  };

  const test = async () => {
    if (!currentDraft) return;
    setTesting(true);
    const activeRequest = ++testRequestId.current;
    setMessage(null);
    setError(null);
    setTestResult(null);
    try {
      const result = await testProxyConnection(currentDraft, [...wslDistros]);
      if (testRequestId.current === activeRequest) {
        setTestResult(result);
      }
    } catch (cause) {
      if (testRequestId.current !== activeRequest) return;
      showOperationError(cause, 'settings.proxy.testError');
    } finally {
      if (testRequestId.current === activeRequest) setTesting(false);
    }
  };

  if (loading) {
    return (
      <div className="space-y-5">
        <Skeleton className="h-14 w-80 max-w-full" />
        <Skeleton className="h-80 w-full max-w-3xl rounded-md" />
      </div>
    );
  }

  if (!draft) {
    return (
      <div className="space-y-3">
        <p role="alert" className="text-sm text-destructive">
          {error ?? t('settings.proxy.loadError')}
        </p>
        <Button
          type="button"
          variant="outline"
          onClick={() => {
            setError(null);
            setLoading(true);
            setLoadAttempt((attempt) => attempt + 1);
          }}
        >
          {t('common.retry')}
        </Button>
      </div>
    );
  }

  return (
    <TooltipProvider>
      <div className="w-full max-w-[720px] pb-2">
        <header>
          <h2 className="text-lg font-semibold leading-6 tracking-normal text-foreground">
            {t('settings.proxy.title')}
          </h2>
          <p className="mt-1 text-sm leading-5 text-muted-foreground">
            {t('settings.proxy.description')}
          </p>
        </header>

        <FlatSection
          id="http-proxy-title"
          title={t('settings.proxy.modeTitle')}
          infoLabel={t('settings.proxy.httpInfo')}
          info={t('settings.proxy.httpInfoDescription')}
        >
          <SettingRow title={t('settings.proxy.connectionMode')}>
            <HttpModeField
              value={draft.mode}
              onChange={(mode) => patchDraft({ mode })}
              t={t}
            />
          </SettingRow>
          {draft.mode === 'custom' ? (
            <SettingRow title={t('settings.proxy.address')}>
              <ProxyInput
                id="http-proxy-address"
                label={t('settings.proxy.httpProxyAddress')}
                value={draft.customProxyUrl ?? ''}
                onChange={(customProxyUrl) => patchDraft({ customProxyUrl })}
              />
            </SettingRow>
          ) : null}
        </FlatSection>

        <FlatSection
          id="native-git-title"
          title={nativeGitLabel}
          infoLabel={t('settings.proxy.gitInfo')}
          info={t('settings.proxy.gitInfoDescription')}
        >
          <SettingRow title={t('settings.proxy.connectionMode')}>
            <GitModeField
              label={nativeGitLabel}
              value={nativeGit.behavior}
              onChange={(behavior) => patchDraft({
                nativeGit: nativeGitSettingsForBehavior(nativeGit, behavior),
              })}
              t={t}
            />
          </SettingRow>
          {nativeGit.behavior === 'useProxy' ? (
            <GitProxyFields
              idPrefix="native-git"
              environmentLabel={nativeGitLabel}
              addressLabel={t('settings.proxy.nativeGitProxyAddress', {
                environment: nativeEnvironmentName,
              })}
              proxyUrl={nativeGit.proxyUrl}
              scope={nativeGit.scope}
              onProxyUrlChange={(proxyUrl) => patchDraft({
                nativeGit: { ...nativeGit, proxyUrl },
              })}
              onScopeChange={(scope) => patchDraft({
                nativeGit: { ...nativeGit, scope },
              })}
              t={t}
            />
          ) : null}
        </FlatSection>

        {wslDistros.map((distro) => {
          const label = `WSL · ${distro} Git`;
          const wslGit = draft.wslGit?.[distro] ?? { behavior: 'useExistingGitConfig' };
          return (
            <FlatSection
              key={distro}
              id={`wsl-git-${distro}`}
              title={label}
              infoLabel={t('settings.proxy.wslInfo', { distro })}
              info={t('settings.proxy.wslInfoDescription', { distro })}
            >
              <SettingRow title={t('settings.proxy.connectionMode')}>
                <WslGitModeField
                  label={label}
                  value={wslGit.behavior}
                  onChange={(behavior) => patchDraft({
                    wslGit: updateRecord(
                      draft.wslGit,
                      distro,
                      wslGitSettingsForBehavior(wslGit, behavior),
                    ),
                  })}
                  t={t}
                />
              </SettingRow>
              {wslGit.behavior === 'useProxy' ? (
                <GitProxyFields
                  idPrefix={`wsl-git-${distro}`}
                  environmentLabel={label}
                  addressLabel={t('settings.proxy.wslGitProxyAddress', { distro })}
                  proxyUrl={wslGit.proxyUrl}
                  scope={wslGit.scope}
                  placeholder="http://172.20.0.1:7890"
                  onProxyUrlChange={(proxyUrl) => patchDraft({
                    wslGit: updateRecord<WslGitProxySettings>(
                      draft.wslGit,
                      distro,
                      { ...wslGit, proxyUrl },
                    ),
                  })}
                  onScopeChange={(scope) => patchDraft({
                    wslGit: updateRecord<WslGitProxySettings>(
                      draft.wslGit,
                      distro,
                      { ...wslGit, scope },
                    ),
                  })}
                  t={t}
                />
              ) : null}
            </FlatSection>
          );
        })}

        <section className="mt-6" aria-labelledby="connection-test-title">
          <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_300px] sm:items-center sm:gap-x-8">
            <SectionHeading
              id="connection-test-title"
              title={t('settings.proxy.testTitle')}
              infoLabel={t('settings.proxy.testInfo')}
              info={t('settings.proxy.testDescription')}
            />
            <Button
              type="button"
              variant="outline"
              className="w-full sm:w-auto sm:justify-self-end"
              disabled={testing || saving}
              onClick={() => void test()}
            >
              {testing ? (
                <LoaderCircle
                  className="size-4 animate-spin motion-reduce:animate-none"
                  aria-hidden="true"
                />
              ) : (
                <Wifi className="size-4" aria-hidden="true" />
              )}
              {testing ? t('settings.proxy.testing') : t('settings.proxy.testConnection')}
            </Button>
          </div>
          <div className="mt-2" aria-live="polite" aria-atomic="true">
            <ConnectionResults
              result={testResult}
              testing={testing}
              nativeEnvironmentName={nativeEnvironmentName}
              wslDistros={wslDistros}
              settings={currentDraft}
              t={t}
            />
          </div>
        </section>

        <footer
          aria-label={t('settings.proxy.actions')}
          className="mt-6 flex min-h-14 flex-col items-stretch justify-between gap-3 border-t border-border/70 pt-4 sm:flex-row sm:items-center sm:gap-4"
        >
          <p
            className={cn(
              'min-h-[18px] min-w-0 text-xs leading-[18px]',
              message ? 'text-success' : 'text-muted-foreground',
            )}
            aria-live="polite"
          >
            {changed ? t('settings.proxy.unsavedChanges') : message ?? ''}
          </p>
          <div className="flex shrink-0 items-center justify-end gap-2">
            <Button
              type="button"
              variant="ghost"
              className="min-w-24"
              disabled={!changed || saving}
              onClick={discardDraft}
            >
              {t('settings.proxy.discard')}
            </Button>
            <Button
              type="button"
              className="min-w-24"
              disabled={!changed || saving || testing || writeBlocked}
              onClick={() => void save()}
            >
              {saving ? (
                <LoaderCircle
                  className="size-4 animate-spin motion-reduce:animate-none"
                  aria-hidden="true"
                />
              ) : null}
              {saving ? t('settings.proxy.saving') : t('settings.proxy.save')}
            </Button>
          </div>
        </footer>
        {error ? <p role="alert" className="mt-2 text-xs text-destructive">{error}</p> : null}
      </div>
    </TooltipProvider>
  );
}

function FlatSection({
  id,
  title,
  infoLabel,
  info,
  children,
}: {
  id: string;
  title: string;
  infoLabel: string;
  info: string;
  children: React.ReactNode;
}) {
  return (
    <section className="mt-6" aria-labelledby={id}>
      <SectionHeading id={id} title={title} infoLabel={infoLabel} info={info} />
      <div className="mt-2 border-y border-border/70 bg-background">{children}</div>
    </section>
  );
}

function SectionHeading({
  id,
  title,
  infoLabel,
  info,
}: {
  id: string;
  title: string;
  infoLabel: string;
  info: string;
}) {
  return (
    <div className="flex items-center gap-1.5">
      <h3 id={id} className="text-sm font-semibold leading-5 text-foreground">{title}</h3>
      <Tooltip>
        <TooltipTrigger asChild>
          <button
            type="button"
            className="inline-flex size-6 items-center justify-center rounded-md text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
            aria-label={infoLabel}
          >
            <Info className="size-3.5" aria-hidden="true" />
          </button>
        </TooltipTrigger>
        <TooltipContent side="top" className="max-w-72 text-xs leading-[18px]">
          {info}
        </TooltipContent>
      </Tooltip>
    </div>
  );
}

function SettingRow({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="grid gap-3 border-t border-border/60 py-3.5 first:border-t-0 sm:grid-cols-[minmax(0,1fr)_300px] sm:items-center sm:gap-x-8">
      <div className="min-w-0 text-sm font-medium leading-5 text-foreground">{title}</div>
      <div className="min-w-0">{children}</div>
    </div>
  );
}

function ConnectionChoice({ title, description }: { title: string; description: string }) {
  return (
    <span className="min-w-0 whitespace-normal">
      <span className="block text-sm leading-5 text-foreground">{title}</span>
      <span className="mt-0.5 block text-xs font-normal leading-[18px] text-muted-foreground">
        {description}
      </span>
    </span>
  );
}

function HttpModeField({
  value,
  onChange,
  t,
}: {
  value: ProxyMode;
  onChange: (value: ProxyMode) => void;
  t: ReturnType<typeof useTranslation>['t'];
}) {
  return (
    <Select value={value} onValueChange={(next) => onChange(next as ProxyMode)}>
      <SelectTrigger aria-label={t('settings.proxy.httpConnectionMode')} className="w-full bg-background">
        <SelectValue>{t(`settings.proxy.mode.${value}`)}</SelectValue>
      </SelectTrigger>
      <SelectContent position="popper" className="w-[var(--radix-select-trigger-width)]">
        {(['direct', 'custom'] as const).map((option) => (
          <SelectItem
            key={option}
            value={option}
            textValue={t(`settings.proxy.mode.${option}`)}
            className="py-2"
          >
            <ConnectionChoice
              title={t(`settings.proxy.mode.${option}`)}
              description={t(`settings.proxy.modeDescriptions.${option}`)}
            />
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

function GitModeField({
  label,
  value,
  onChange,
  t,
}: {
  label: string;
  value: GitProxyBehavior;
  onChange: (value: GitProxyBehavior) => void;
  t: ReturnType<typeof useTranslation>['t'];
}) {
  return (
    <Select value={value} onValueChange={(next) => onChange(next as GitProxyBehavior)}>
      <SelectTrigger aria-label={label} className="w-full bg-background">
        <SelectValue>{t(`settings.proxy.gitBehavior.${value}`)}</SelectValue>
      </SelectTrigger>
      <SelectContent position="popper" className="w-[var(--radix-select-trigger-width)]">
        {(['useExistingGitConfig', 'useProxy'] as const).map((option) => (
          <SelectItem
            key={option}
            value={option}
            textValue={t(`settings.proxy.gitBehavior.${option}`)}
            className="py-2"
          >
            <ConnectionChoice
              title={t(`settings.proxy.gitBehavior.${option}`)}
              description={t(`settings.proxy.gitBehaviorDescriptions.${option}`)}
            />
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

function WslGitModeField({
  label,
  value,
  onChange,
  t,
}: {
  label: string;
  value: WslGitProxyBehavior;
  onChange: (value: WslGitProxyBehavior) => void;
  t: ReturnType<typeof useTranslation>['t'];
}) {
  return (
    <Select value={value} onValueChange={(next) => onChange(next as WslGitProxyBehavior)}>
      <SelectTrigger aria-label={label} className="w-full bg-background">
        <SelectValue>{t(`settings.proxy.wslBehavior.${value}`)}</SelectValue>
      </SelectTrigger>
      <SelectContent position="popper" className="w-[var(--radix-select-trigger-width)]">
        {(['followNativeGit', 'useExistingGitConfig', 'useProxy'] as const).map((option) => (
          <SelectItem
            key={option}
            value={option}
            textValue={t(`settings.proxy.wslBehavior.${option}`)}
            className="py-2"
          >
            <ConnectionChoice
              title={t(`settings.proxy.wslBehavior.${option}`)}
              description={t(`settings.proxy.wslBehaviorDescriptions.${option}`)}
            />
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

function GitProxyFields({
  idPrefix,
  environmentLabel,
  addressLabel,
  proxyUrl,
  scope,
  placeholder = 'http://127.0.0.1:7890',
  onProxyUrlChange,
  onScopeChange,
  t,
}: {
  idPrefix: string;
  environmentLabel: string;
  addressLabel: string;
  proxyUrl: string;
  scope: GitProxyScope;
  placeholder?: string;
  onProxyUrlChange: (value: string) => void;
  onScopeChange: (value: GitProxyScope) => void;
  t: ReturnType<typeof useTranslation>['t'];
}) {
  return (
    <>
      <SettingRow title={t('settings.proxy.address')}>
        <ProxyInput
          id={`${idPrefix}-proxy-address`}
          label={addressLabel}
          value={proxyUrl}
          placeholder={placeholder}
          onChange={onProxyUrlChange}
        />
      </SettingRow>
      <SettingRow title={t('settings.proxy.scopeTitle')}>
        <ScopeField
          label={t('settings.proxy.scopeLabel', { environment: environmentLabel })}
          value={scope}
          onChange={onScopeChange}
          t={t}
        />
      </SettingRow>
    </>
  );
}

function ProxyInput({
  id,
  label,
  value,
  placeholder = 'http://127.0.0.1:7890',
  onChange,
}: {
  id: string;
  label: string;
  value: string;
  placeholder?: string;
  onChange: (value: string) => void;
}) {
  return (
    <Input
      id={id}
      type="url"
      inputMode="url"
      autoComplete="off"
      spellCheck={false}
      required
      aria-required="true"
      aria-label={label}
      value={value}
      placeholder={placeholder}
      onChange={(event) => onChange(event.target.value)}
    />
  );
}

function ScopeField({
  label,
  value,
  onChange,
  t,
}: {
  label: string;
  value: GitProxyScope;
  onChange: (value: GitProxyScope) => void;
  t: ReturnType<typeof useTranslation>['t'];
}) {
  return (
    <Select value={value} onValueChange={(next) => onChange(next as GitProxyScope)}>
      <SelectTrigger aria-label={label} className="w-full bg-background">
        <SelectValue>{t(`settings.proxy.scope.${value}`)}</SelectValue>
      </SelectTrigger>
      <SelectContent position="popper" className="w-[var(--radix-select-trigger-width)]">
        <SelectItem value="githubOnly">{t('settings.proxy.scope.githubOnly')}</SelectItem>
        <SelectItem value="allHttpHttps">{t('settings.proxy.scope.allHttpHttps')}</SelectItem>
      </SelectContent>
    </Select>
  );
}

function ConnectionResults({
  result,
  testing,
  nativeEnvironmentName,
  wslDistros,
  settings,
  t,
}: {
  result: ProxyConnectionTestResult | null;
  testing: boolean;
  nativeEnvironmentName: string;
  wslDistros: string[];
  settings: NetworkProxySettings | null;
  t: ReturnType<typeof useTranslation>['t'];
}) {
  const entries: Array<[
    string,
    string,
    string,
    ProxyConnectionTestResult['onlineServices'] | null,
    boolean,
  ]> = [
    [
      'onlineServices',
      HTTP_CONNECTION_TEST_TARGET,
      t('settings.proxy.test.targets.onlineServices'),
      result?.onlineServices ?? null,
      false,
    ],
    [
      'nativeGit',
      GIT_CONNECTION_TEST_TARGET,
      `${nativeEnvironmentName} Git`,
      result?.nativeGit ?? null,
      false,
    ],
  ];
  for (const distro of wslDistros) {
    const behavior = settings?.wslGit?.[distro]?.behavior ?? 'useExistingGitConfig';
    entries.push([
      `wslGit:${distro}`,
      GIT_CONNECTION_TEST_TARGET,
      `WSL · ${distro} Git`,
      result?.wslGitByDistro[distro] ?? null,
      behavior === 'useProxy'
        || (behavior === 'followNativeGit' && settings?.nativeGit?.behavior === 'useProxy'),
    ]);
  }

  return (
    <div className="divide-y divide-border/60 border-y border-border/70">
      {entries.map(([key, target, label, probe, wslProxyConfigured]) => {
        const status = testing ? 'testing' : probe?.status ?? 'idle';
        const succeeded = status === 'succeeded';
        const skipped = status === 'idle';
        const showWslProxyHint = key.startsWith('wslGit:')
          && probe?.reasonCode === 'git_network'
          && wslProxyConfigured;
        return (
          <div
            key={key}
            className="grid min-h-12 gap-2 py-2.5 text-xs sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center sm:gap-x-6"
          >
            <span className="min-w-0">
              <span className="block break-all text-foreground">{target}</span>
              <span className="mt-0.5 block text-muted-foreground">{label}</span>
            </span>
            <span className="min-w-0 sm:text-right">
              <span className={cn(
                'inline-flex items-center gap-1.5',
                succeeded
                  ? 'text-success'
                  : skipped || testing
                    ? 'text-muted-foreground'
                    : 'text-destructive',
              )}>
                {testing
                  ? <LoaderCircle className="size-3.5 animate-spin motion-reduce:animate-none" aria-hidden="true" />
                  : succeeded
                    ? <CheckCircle2 className="size-3.5" aria-hidden="true" />
                    : skipped
                      ? <CircleMinus className="size-3.5" aria-hidden="true" />
                      : <XCircle className="size-3.5" aria-hidden="true" />}
                {t(`settings.proxy.test.status.${status}`)}
                {probe && !skipped && !testing ? (
                  <span className="text-muted-foreground">{probe.elapsedMs} ms</span>
                ) : null}
              </span>
              {!succeeded && !skipped && probe?.reasonCode ? (
                <span className="mt-0.5 block break-words text-muted-foreground">
                  {t(`settings.proxy.test.reasons.${probe.reasonCode}`)}
                </span>
              ) : null}
              {showWslProxyHint ? (
                <span className="mt-0.5 block break-words text-muted-foreground">
                  {t('settings.proxy.test.wslProxyHint')}
                </span>
              ) : null}
            </span>
          </div>
        );
      })}
    </div>
  );
}
