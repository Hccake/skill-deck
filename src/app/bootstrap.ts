import i18n from '@/i18n';
import { applyPersistedAppearance, useSettingsStore } from '@/stores/settings';

/** 在每个 WebView 首次渲染前恢复共享的主题和语言偏好。 */
export function bootstrapAppPreferences(): void {
  const { theme, locale } = useSettingsStore.getState();
  applyPersistedAppearance(theme);
  i18n.changeLanguage(locale);
}
