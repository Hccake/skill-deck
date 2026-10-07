import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { useRegisterUnsavedChanges } from '@/lifecycle/unsaved-changes-context';
import { useTranslationSettings, translationDraft } from '@/stores/translation';
import { clearTranslationCache, getTranslationCacheInfo, testTranslationService } from '@/hooks/useTauriApi';
import type { TranslationSettingsDraft, TranslationProfileDraft, TranslationCacheInfo } from '@/bindings';
import { useSettingsStore, translationColors, type TranslationColor } from '@/stores/settings';
import '@/components/skills/detail/SkillDetail.css';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';

type Preferences = Pick<TranslationSettingsDraft, 'service' | 'autoTranslate'>;

export function TranslationSettingsPage() {
  const { t } = useTranslation();
  const writeBlocked = useBusinessWriteBlocked();
  const locale = useSettingsStore(s => s.locale);
  const translationColor = useSettingsStore(s => s.translationColor);
  const setTranslationColor = useSettingsStore(s => s.setTranslationColor);
  const [colorDraft, setColorDraft] = useState<TranslationColor | null>(null);
  const currentColor = colorDraft ?? translationColor;
  const settings = useTranslationSettings(s => s.settings);
  const load = useTranslationSettings(s => s.load);
  const save = useTranslationSettings(s => s.save);
  const loadError = useTranslationSettings(s => s.error);
  const [draft, setDraft] = useState<Preferences | null>(null);
  const [profile, setProfile] = useState<TranslationProfileDraft | null>(null);
  const [profileDirty, setProfileDirty] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [profileError, setProfileError] = useState<string | null>(null);
  const [profileMessage, setProfileMessage] = useState<string | null>(null);
  const [cacheMessage, setCacheMessage] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!message) return;
    const timer = setTimeout(() => setMessage(null), 2500);
    return () => clearTimeout(timer);
  }, [message]);
  const [cache, setCache] = useState<TranslationCacheInfo | null>(null);
  useEffect(() => { void load(); void getTranslationCacheInfo().then(setCache).catch(() => {}); }, [load]);
  const current = draft ?? settings;
  const discard = useCallback(() => { setDraft(null); setColorDraft(null); setProfile(null); setProfileDirty(false); setError(null); setMessage(null); }, []);
  const dirty = draft !== null || colorDraft !== null;
  const registration = useMemo(() => ({ dirty: dirty || profileDirty, discard }), [dirty, profileDirty, discard]);
  useRegisterUnsavedChanges(registration);
  const errorText = (failure: unknown) => t(`translation.errors.${typeof failure === 'string' ? failure : 'network'}`, { defaultValue: t('translation.failed') });
  const perform = async (action: () => Promise<void>, onError: (message: string | null) => void) => {
    setBusy(true); onError(null);
    try { await action(); } catch (failure) { onError(errorText(failure)); }
    finally { setBusy(false); }
  };
  if (!current || !settings) return <div role="status" className="space-y-3">
    <p>{loadError ? t('translation.errors.invalidSettings') : t('common.loading')}</p>
    {loadError ? <Button onClick={() => void load()}>{t('translation.retry')}</Button> : null}
  </div>;
  const serviceValue = current.service.kind === 'openAiCompatible' ? `profile:${current.service.profileId}` : current.service.kind;
  const selectedProfile = current.service.kind === 'openAiCompatible' ? settings.profiles.find(p => `profile:${p.id}` === serviceValue) : null;
  const update = (patch: Partial<Preferences>) => {
    setDraft({ service: current.service, autoTranslate: current.autoTranslate, ...patch });
    setMessage(null); setError(null);
  };
  const openProfile = (value: TranslationProfileDraft) => {
    setProfile(value); setProfileDirty(false); setProfileError(null); setProfileMessage(null);
  };
  const updateProfile = (patch: Partial<TranslationProfileDraft>) => {
    if (profile) { setProfile({ ...profile, ...patch }); setProfileDirty(true); setProfileError(null); setProfileMessage(null); }
  };
  const cacheBytes = cache?.bytes ?? 0;
  const cacheUnit = cacheBytes < 1024 ? 'B' : cacheBytes < 1024 * 1024 ? 'KiB' : 'MiB';
  const cacheDivisor = cacheUnit === 'B' ? 1 : cacheUnit === 'KiB' ? 1024 : 1024 * 1024;
  const cacheSize = `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(cacheBytes / cacheDivisor)} ${cacheUnit}`;
  return <div className="@container max-w-xl">
    <h2 className="mb-7 text-lg font-semibold">{t('translation.settings')}</h2>
    <section className="space-y-4" aria-labelledby="translation-services-heading">
      <h3 id="translation-services-heading" className="text-sm font-semibold">{t('translation.servicesHeading')}</h3>
      <div className="grid min-w-0 gap-2 @min-[480px]:grid-cols-[9rem_minmax(0,1fr)] @min-[480px]:items-center @min-[480px]:gap-x-6">
        <Label htmlFor="translation-service" className="text-sm font-normal leading-5">{t('translation.currentService')}</Label>
        <Select disabled={busy} value={serviceValue} onValueChange={value => update({ service: value === 'google' || value === 'microsoft' ? { kind: value } : { kind: 'openAiCompatible', profileId: value.slice('profile:'.length) } })}>
          <SelectTrigger id="translation-service" className="w-full min-w-0">
            <SelectValue><span className="truncate">{selectedProfile?.name || t(`translation.${current.service.kind}`)}</span></SelectValue>
          </SelectTrigger>
          <SelectContent position="popper" className="max-w-[calc(100vw-2rem)]">
            {settings.profiles.length > 0 ? <SelectGroup>
              <SelectLabel>{t('translation.languageModel')}</SelectLabel>
              {settings.profiles.map(p => <SelectItem key={p.id} value={`profile:${p.id}`} textValue={p.name}>
                <span className="block min-w-0 max-w-64" title={p.name}><span className="block truncate">{p.name}</span><span className="block truncate text-xs text-muted-foreground">{p.model}</span></span>
              </SelectItem>)}
            </SelectGroup> : null}
            <SelectGroup>
              <SelectLabel>{t('translation.ordinary')}</SelectLabel>
              <SelectItem value="microsoft">{t('translation.microsoft')}</SelectItem>
              <SelectItem value="google">{t('translation.google')}</SelectItem>
            </SelectGroup>
          </SelectContent>
        </Select>
      </div>
      <div className="grid min-w-0 gap-2 @min-[480px]:grid-cols-[9rem_minmax(0,1fr)] @min-[480px]:items-center @min-[480px]:gap-x-6">
        <span className="text-sm font-normal leading-5 @min-[480px]:self-start @min-[480px]:pt-1.5">{t('translation.customServices')}</span>
        <div className="min-w-0">
          <Button variant="ghost" size="sm" className="-ml-2 h-8 font-normal" disabled={busy || writeBlocked} onClick={() => openProfile({ id: crypto.randomUUID(), name: '', baseUrl: 'https://api.openai.com/v1', model: '', key: { action: 'clear' } })}><Plus aria-hidden="true" />{t('translation.addProfile')}</Button>
          {settings.profiles.map(p => <div key={p.id} className="flex items-center justify-between gap-3 py-2 text-sm">
            <div className="min-w-0"><p className="truncate" title={p.name}>{p.name}</p><p className="truncate text-xs text-muted-foreground" title={p.model}>{p.model}</p></div>
            <div className="flex shrink-0 gap-1"><Button variant="ghost" size="sm" disabled={busy || writeBlocked} onClick={() => openProfile({ id: p.id, name: p.name, baseUrl: p.baseUrl, model: p.model, key: { action: 'keep' } })}>{t('translation.edit')}</Button>
              <Button variant="ghost" size="sm" disabled={busy || writeBlocked} onClick={() => {
                setMessage(null);
                if ((settings.service.kind === 'openAiCompatible' && settings.service.profileId === p.id) || (current.service.kind === 'openAiCompatible' && current.service.profileId === p.id)) {
                  setError(t('translation.switchBeforeDelete')); return;
                }
                void perform(async () => { await save({ ...translationDraft(settings), profiles: translationDraft(settings).profiles.filter(item => item.id !== p.id) }); }, setError);
              }}>{t('translation.delete')}</Button></div>
          </div>)}
        </div>
      </div>
    </section>
    <section className="mt-6 space-y-4 border-t pt-5" aria-labelledby="translation-reading-heading">
      <div>
        <h3 id="translation-reading-heading" className="text-sm font-semibold">{t('translation.readingHeading')}</h3>
        <p className="mt-1 text-xs text-muted-foreground">{t('translation.targetHint')}</p>
      </div>
      <div className="grid min-w-0 gap-2 @min-[480px]:grid-cols-[9rem_minmax(0,1fr)] @min-[480px]:items-center @min-[480px]:gap-x-6">
        <Label htmlFor="translation-auto" className="text-sm font-normal leading-5">{t('translation.auto')}</Label>
        <Switch id="translation-auto" checked={current.autoTranslate} disabled={busy} onCheckedChange={value => update({ autoTranslate: value })} />
      </div>
      <div className="grid min-w-0 gap-2 @min-[480px]:grid-cols-[9rem_minmax(0,1fr)] @min-[480px]:items-center @min-[480px]:gap-x-6">
        <Label htmlFor="translation-color" className="text-sm font-normal leading-5 @min-[480px]:self-start @min-[480px]:pt-2">{t('translation.color')}</Label>
        <div className="min-w-0 space-y-3">
          <Select disabled={busy} value={currentColor} onValueChange={value => {
            setColorDraft(value === translationColor ? null : value as TranslationColor); setMessage(null); setError(null);
          }}>
            <SelectTrigger id="translation-color" className="w-full min-w-0"><SelectValue /></SelectTrigger>
            <SelectContent position="popper">
              {translationColors.map(color => <SelectItem key={color} value={color}>
                <span aria-hidden="true" data-translation-color={color} className="translation-color-swatch size-3 shrink-0 rounded-full" />
                {t(`translation.colors.${color}`)}
              </SelectItem>)}
            </SelectContent>
          </Select>
          <div role="region" aria-label={t('translation.preview')} data-translation-color={currentColor} className="text-[13px] leading-[22px] text-foreground/80">
            <p lang="en">{t('translation.previewOriginal')}</p>
            <p lang="zh-CN" className="skill-translation">{t('translation.previewTranslated')}</p>
          </div>
        </div>
      </div>
    </section>
    <section className="mt-6 border-t pt-5" aria-label={t('translation.cache')}>
      <div className="grid min-w-0 gap-2 @min-[480px]:grid-cols-[9rem_minmax(0,1fr)] @min-[480px]:items-center @min-[480px]:gap-x-6">
        <h3 className="text-sm font-normal leading-5">{t('translation.cache')}</h3>
        <div className="flex min-w-0 flex-wrap items-center justify-between gap-x-3 gap-y-1">
          <p className="text-xs text-muted-foreground" role="status">{cache ? t('translation.cacheSize', { count: cache.entries, size: cacheSize }) : '—'}</p>
          <Button variant="ghost" size="sm" className="h-8 font-normal" disabled={busy} onClick={() => { setCacheMessage(null); void perform(async () => { setCache(await clearTranslationCache()); setCacheMessage(t('translation.cacheCleared')); }, setCacheMessage); }}>{t('translation.clearCache')}</Button>
        </div>
      </div>
      {cacheMessage ? <p role="status" className="mt-2 text-xs text-muted-foreground">{cacheMessage}</p> : null}
    </section>
    <div className="mt-6 border-t pt-4">
      {error ? <p role="alert" className="mb-3 text-sm text-destructive">{error}</p> : null}
      <div role="group" aria-label={t('translation.saveActions')} className="flex min-h-8 flex-wrap items-center gap-2">
        <Button variant={dirty ? 'default' : 'secondary'} size="sm" disabled={busy || !dirty || (Boolean(draft) && writeBlocked)} onClick={() => { setMessage(null); void perform(async () => {
          if (draft) await save({ ...translationDraft(settings), ...draft });
          if (colorDraft !== null) setTranslationColor(colorDraft);
          setDraft(null); setColorDraft(null); setMessage(t('translation.saved'));
        }, setError); }}>{t('translation.save')}</Button>
        <Button variant="ghost" size="sm" disabled={busy || !dirty} onClick={discard}>{t('translation.discard')}</Button>
        <span role="status" className="text-xs text-muted-foreground">{dirty ? t('translation.unsaved') : message}</span>
      </div>
    </div>
    <Dialog open={profile !== null} onOpenChange={open => { if (!open && !busy) { setProfile(null); setProfileDirty(false); } }}>
      <DialogContent dismissible={!busy} closeLabel={t('common.close')} className="max-h-[85vh] overflow-y-auto">
        <DialogHeader><DialogTitle>{t(settings.profiles.some(p => p.id === profile?.id) ? 'translation.editProfile' : 'translation.addProfile')}</DialogTitle><DialogDescription>{t('translation.compatibleHint')}</DialogDescription></DialogHeader>
        {profile ? <form className="space-y-4" onSubmit={event => {
          event.preventDefault();
          if (busy || writeBlocked) return;
          setProfileMessage(null);
          void perform(async () => {
            const savedDraft = translationDraft(settings);
            const profiles = savedDraft.profiles.some(p => p.id === profile.id) ? savedDraft.profiles.map(p => p.id === profile.id ? profile : p) : [...savedDraft.profiles, profile];
            await save({ ...savedDraft, profiles }); setProfile(null); setProfileDirty(false);
          }, setProfileError);
        }}>
          {(['name', 'baseUrl', 'model'] as const).map(field => <div className="space-y-2" key={field}>
            <Label htmlFor={`translation-${field}`}>{t(`translation.${field === 'name' ? 'profileName' : field}`)}</Label>
            <Input id={`translation-${field}`} name={field} type={field === 'baseUrl' ? 'url' : 'text'} value={profile[field]} disabled={busy} autoComplete="off" spellCheck={false} onChange={event => updateProfile({ [field]: event.target.value })} />
          </div>)}
          <div className="space-y-2"><Label htmlFor="translation-key">{t('translation.apiKey')}</Label>
            <Input id="translation-key" name="apiKey" type="password" autoComplete="new-password" disabled={busy}
              placeholder={profile.key.action === 'keep' && settings.profiles.find(p => p.id === profile.id)?.hasApiKey ? t('translation.keyConfigured') : t('translation.keyOptional')}
              value={profile.key.action === 'replace' ? profile.key.value : ''}
              onChange={event => updateProfile({ key: event.target.value ? { action: 'replace', value: event.target.value } : { action: settings.profiles.find(p => p.id === profile.id)?.hasApiKey ? 'keep' : 'clear' } })} />
            {settings.profiles.find(p => p.id === profile.id)?.hasApiKey && profile.key.action !== 'clear' ? <Button type="button" variant="link" size="sm" className="h-auto px-0" disabled={busy} onClick={() => updateProfile({ key: { action: 'clear' } })}>{t('translation.clearKey')}</Button> : null}
          </div>
          <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => { setProfileMessage(null); void perform(async () => { setProfileMessage(await testTranslationService({ kind: 'openAiCompatible', profileId: profile.id }, profile)); }, setProfileError); }}>{t('translation.test')}</Button>
          {profileError ? <p role="alert" className="text-sm text-destructive">{profileError}</p> : null}
          {profileMessage ? <p role="status" className="text-sm">{profileMessage}</p> : null}
          <DialogFooter><Button type="button" variant="ghost" disabled={busy} onClick={() => { setProfile(null); setProfileDirty(false); }}>{t('common.cancel')}</Button><Button type="submit" disabled={busy || writeBlocked}>{t('translation.saveProfile')}</Button></DialogFooter>
        </form> : null}
      </DialogContent>
    </Dialog>
  </div>;
}
