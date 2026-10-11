import '@/test-utils';
import { beforeEach, expect, it, vi } from 'vitest';
import { fireEvent, render as testingLibraryRender, screen, waitFor } from '@testing-library/react';
import { TooltipProvider } from '@/components/ui/tooltip';

const render = (ui: React.ReactNode) => testingLibraryRender(ui, { wrapper: TooltipProvider });
import type { TranslationSettings } from '@/bindings';
import { TranslationSettingsPage } from '../TranslationSettingsPage';
import { useSettingsStore } from '@/stores/settings';
import { useTranslationSettings } from '@/stores/translation';

const api = vi.hoisted(() => ({
  settings: { revision: 'one', serviceRevision: 'one', service: { kind: 'microsoft' }, profiles: [], autoTranslate: false, noticeAccepted: true } as TranslationSettings,
  cache: { bytes: 0, entries: 0 },
  test: vi.fn(async (..._args: unknown[]) => '示例译文'), save: vi.fn(),
}));
vi.mock('@/hooks/useTauriApi', () => ({
  getTranslationSettings: async () => api.settings,
  saveTranslationSettings: (draft: unknown) => api.save(draft),
  testTranslationService: (...args: unknown[]) => api.test(...args),
  getTranslationCacheInfo: async () => api.cache,
  clearTranslationCache: async () => ({ bytes: 0, entries: 0 }),
}));
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string, values?: { count: number; size: string }) => key === 'translation.cacheSize' ? `${values?.count} · ${values?.size}` : key }) }));
beforeEach(() => {
  HTMLElement.prototype.scrollIntoView = vi.fn();
  useSettingsStore.setState({ translationColor: 'teal' });
  document.documentElement.dataset.translationColor = 'teal';
  useTranslationSettings.setState({ settings: null, error: null });
  api.settings = { revision: 'one', serviceRevision: 'one', service: { kind: 'microsoft' }, profiles: [], autoTranslate: false, noticeAccepted: true };
  api.cache = { bytes: 0, entries: 0 };
  api.test.mockClear(); api.save.mockReset();
});
it('默认只展示服务选择，不展示分类卡片、返回入口或连接测试', async () => {
  render(<TranslationSettingsPage />);
  await screen.findByRole('combobox', { name: 'translation.currentService' });
  expect(screen.queryByText('translation.ordinary')).toBeNull();
  expect(screen.queryByText('translation.freeHint')).toBeNull();
  expect(screen.queryByText('translation.returnToDetail')).toBeNull();
  expect(screen.queryByRole('button', { name: 'translation.test' })).toBeNull();
});
it('取消新增配置不产生页面草稿，连接测试不保存', async () => {
  render(<TranslationSettingsPage />);
  fireEvent.click(await screen.findByRole('button', { name: 'translation.addProfile' }));
  expect(screen.getByRole('dialog')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'translation.test' }));
  await screen.findByText('示例译文');
  expect(api.save).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'common.cancel' }));
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(screen.queryByText('translation.unsaved')).toBeNull();
});
it('保存失败时在弹窗中保留服务草稿', async () => {
  api.save.mockRejectedValue('credentialUnavailable');
  render(<TranslationSettingsPage />);
  fireEvent.click(await screen.findByRole('button', { name: 'translation.addProfile' }));
  fireEvent.change(screen.getByLabelText('translation.profileName'), { target: { value: 'My service' } });
  fireEvent.click(screen.getByRole('button', { name: 'translation.saveProfile' }));
  await waitFor(() => expect(api.save).toHaveBeenCalled());
  expect((screen.getByLabelText('translation.profileName') as HTMLInputElement).value).toBe('My service');
  expect(await screen.findByText('translation.errors.credentialUnavailable')).toBeTruthy();
});
it('保存服务不应用未保存的阅读偏好，随后保存偏好使用最新配置版本', async () => {
  api.save.mockImplementation(async draft => ({ ...api.settings, ...draft, revision: 'two', profiles: draft.profiles.map((p: object) => ({ ...p, hasApiKey: false })) }));
  render(<TranslationSettingsPage />);
  fireEvent.click(await screen.findByRole('switch'));
  fireEvent.click(screen.getByRole('button', { name: 'translation.addProfile' }));
  fireEvent.change(screen.getByLabelText('translation.profileName'), { target: { value: 'My service' } });
  fireEvent.click(screen.getByRole('button', { name: 'translation.saveProfile' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  expect(api.save.mock.calls[0][0]).toMatchObject({ autoTranslate: false, service: { kind: 'microsoft' }, profiles: [{ name: 'My service' }] });
  expect(screen.getByRole('switch').getAttribute('aria-checked')).toBe('true');
  fireEvent.click(screen.getByRole('button', { name: 'translation.save' }));
  await waitFor(() => expect(api.save).toHaveBeenCalledTimes(2));
  expect(api.save.mock.calls[1][0]).toMatchObject({ expectedRevision: 'two', autoTranslate: true, profiles: [{ name: 'My service', key: { action: 'keep' } }] });
});

it('键盘打开分组菜单并切换服务，选择本身不保存配置', async () => {
  render(<TranslationSettingsPage />);
  const select = await screen.findByRole('combobox', { name: 'translation.currentService' });
  fireEvent.keyDown(select, { key: 'ArrowDown' });
  const google = await screen.findByRole('option', { name: 'translation.google' });
  expect(screen.queryByText('translation.languageModel')).toBeNull();
  fireEvent.click(google);
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(api.save).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'translation.save' }));
  await waitFor(() => expect(api.save).toHaveBeenCalled());
  expect(api.save.mock.calls[0][0].service).toEqual({ kind: 'google' });
});
it('编辑已有服务保留密钥，当前服务必须切换并保存后才能删除', async () => {
  api.settings.profiles = [{ id: 'custom', name: 'Custom', baseUrl: 'https://example.com/v1', model: 'model', hasApiKey: true }];
  api.settings.service = { kind: 'openAiCompatible', profileId: 'custom' };
  api.save.mockRejectedValue('credentialUnavailable');
  render(<TranslationSettingsPage />);
  fireEvent.click(await screen.findByRole('button', { name: 'translation.delete' }));
  expect(await screen.findByRole('alert')).toHaveProperty('textContent', 'translation.switchBeforeDelete');
  expect(api.save).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'translation.edit' }));
  expect((screen.getByLabelText('translation.apiKey') as HTMLInputElement).value).toBe('');
  fireEvent.change(screen.getByLabelText('translation.profileName'), { target: { value: 'Renamed' } });
  fireEvent.click(screen.getByRole('button', { name: 'translation.saveProfile' }));
  await waitFor(() => expect(api.save).toHaveBeenCalled());
  expect(api.save.mock.calls[0][0].profiles[0].key).toEqual({ action: 'keep' });
});

it('配色只更新预览，保存后持久化且不保存翻译服务', async () => {
  render(<TranslationSettingsPage />);
  const select = await screen.findByRole('combobox', { name: 'translation.color' });
  fireEvent.keyDown(select, { key: 'ArrowDown' });
  fireEvent.click(await screen.findByRole('option', { name: 'translation.colors.blue' }));
  expect(screen.getByRole('region', { name: 'translation.preview' }).getAttribute('data-translation-color')).toBe('blue');
  expect(useSettingsStore.getState().translationColor).toBe('teal');
  fireEvent.click(screen.getByRole('button', { name: 'translation.discard' }));
  expect(screen.getByRole('region', { name: 'translation.preview' }).getAttribute('data-translation-color')).toBe('teal');
  fireEvent.keyDown(select, { key: 'ArrowDown' });
  fireEvent.click(await screen.findByRole('option', { name: 'translation.colors.blue' }));
  fireEvent.click(screen.getByRole('button', { name: 'translation.save' }));
  await screen.findByText('translation.saved');
  expect(useSettingsStore.getState().translationColor).toBe('blue');
  expect(document.documentElement.dataset.translationColor).toBe('blue');
  expect(api.save).not.toHaveBeenCalled();
  expect(api.test).not.toHaveBeenCalled();
});
it('服务保存失败时不应用同时修改的配色', async () => {
  api.save.mockRejectedValue('network');
  render(<TranslationSettingsPage />);
  const select = await screen.findByRole('combobox', { name: 'translation.color' });
  fireEvent.keyDown(select, { key: 'ArrowDown' });
  fireEvent.click(await screen.findByRole('option', { name: 'translation.colors.inherit' }));
  fireEvent.click(screen.getByRole('switch'));
  fireEvent.click(screen.getByRole('button', { name: 'translation.save' }));
  await screen.findByText('translation.errors.network');
  expect(useSettingsStore.getState().translationColor).toBe('teal');
  expect(screen.getByRole('region', { name: 'translation.preview' }).getAttribute('data-translation-color')).toBe('inherit');
});

it('配色菜单提供五种颜色及跟随原文，每个选项保留可读名称', async () => {
  render(<TranslationSettingsPage />);
  fireEvent.keyDown(await screen.findByRole('combobox', { name: 'translation.color' }), { key: 'ArrowDown' });
  expect((await screen.findAllByRole('option')).map(option => option.textContent)).toEqual(
    ['teal', 'blue', 'purple', 'brown', 'gray', 'inherit'].map(color => `translation.colors.${color}`),
  );
});

it('服务选择与自定义配置在同一分组，阅读偏好另成一组', async () => {
  render(<TranslationSettingsPage />);
  const service = await screen.findByRole('combobox', { name: 'translation.currentService' });
  expect(service.closest('section')?.contains(screen.getByRole('button', { name: 'translation.addProfile' }))).toBe(true);
  const color = screen.getByRole('combobox', { name: 'translation.color' });
  expect(color.closest('section')).not.toBe(service.closest('section'));
  expect(color.closest('section')?.contains(screen.getByRole('region', { name: 'translation.preview' }))).toBe(true);
});
it.each([[128, '128 B'], [48 * 1024, '48 KiB'], [2 * 1024 * 1024, '2 MiB']])('缓存容量 %s 使用合适单位展示', async (bytes, size) => {
  api.cache = { bytes: Number(bytes), entries: 213 };
  render(<TranslationSettingsPage />);
  expect(await screen.findByText(`213 · ${size}`)).toBeTruthy();
});
