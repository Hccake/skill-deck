import { create } from 'zustand';
import type { TranslationSettings, TranslationSettingsDraft } from '@/bindings';
import { getTranslationSettings, saveTranslationSettings } from '@/hooks/useTauriApi';

export function translationDraft(settings: TranslationSettings): TranslationSettingsDraft {
  return { expectedRevision: settings.revision, service: settings.service,
    autoTranslate: settings.autoTranslate, noticeAccepted: settings.noticeAccepted,
    profiles: settings.profiles.map(profile => ({ id: profile.id, name: profile.name,
      baseUrl: profile.baseUrl, model: profile.model, key: { action: 'keep' } })) };
}
let loading: Promise<void> | null = null;
export const useTranslationSettings = create<{
  settings: TranslationSettings | null;
  error: unknown;
  load: () => Promise<void>;
  save: (draft: TranslationSettingsDraft) => Promise<TranslationSettings>;
}>((set) => ({
  settings: null, error: null,
  load: () => {
    if (!loading) loading = Promise.resolve().then(() => getTranslationSettings()).then(settings => { set({ settings, error: null }); })
      .catch(error => { set({ error }); }).finally(() => { loading = null; });
    return loading;
  },
  save: async draft => {
    const settings = await saveTranslationSettings(draft);
    set({ settings, error: null });
    return settings;
  },
}));

// 阅读意图只在本次运行内保存，按对象与内容区分，不持久化正文。
export const translationPreferences = new Map<string, boolean>();
export function rememberTranslation(key: string, enabled: boolean) {
  translationPreferences.delete(key);
  translationPreferences.set(key, enabled);
  if (translationPreferences.size > 200) translationPreferences.delete(translationPreferences.keys().next().value!);
}
