import '@/test-utils';
import { beforeEach, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { TooltipProvider } from '@/components/ui/tooltip';
import { DetailTranslationProvider, TranslatedBlock, TranslationActions, TranslationStatus } from '../DetailTranslation';
import { clearReadingReturn } from '@/lib/translation/navigation';
import { translationPreferences, useTranslationSettings } from '@/stores/translation';
import type { TranslationResult, TranslationUnit } from '@/bindings';
import { DetailBody } from '../DetailPrimitives';
import { DetailHeader } from '../DetailPrimitives';
import { MemoryRouter } from 'react-router-dom';
import userEvent from '@testing-library/user-event';

const api = vi.hoisted(() => ({
  settings: { revision: 'one', serviceRevision: 'one', service: { kind: 'microsoft' }, profiles: [], autoTranslate: false, noticeAccepted: true },
  translate: vi.fn(), cancel: vi.fn(async (_id: string) => {}),
}));
vi.mock('@/hooks/useTauriApi', () => ({
  getTranslationSettings: async () => api.settings,
  saveTranslationSettings: async () => api.settings,
  beginTranslation: async () => 'session',
  cancelTranslation: (id: string) => api.cancel(id),
  translateUnits: (_id: string, units: TranslationUnit[]) => api.translate(units),
}));
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

beforeEach(() => {
  useTranslationSettings.setState({ settings: null, error: null });
  translationPreferences.clear();
  clearReadingReturn();
  api.settings.revision = 'one'; api.settings.serviceRevision = 'one'; api.settings.service = { kind: 'microsoft' };
  api.cancel.mockClear();
  api.translate.mockReset();
});
function mount() {
  return render(<TooltipProvider><a href="/settings" onClick={event => event.preventDefault()}>settings navigation</a><DetailTranslationProvider documentKey="skill-one" context="Example">
    <TranslationActions /><TranslationStatus /><TranslatedBlock as="p">Read the guide</TranslatedBlock>
  </DetailTranslationProvider></TooltipProvider>);
}
it('翻译只保留标题旁的图标开关，不占用业务操作组或提供更多菜单', async () => {
  const user = userEvent.setup();
  api.translate.mockResolvedValue([]);
  render(<TooltipProvider><DetailTranslationProvider documentKey="header" context="Example">
    <DetailHeader name="Example"><button>Install</button></DetailHeader>
  </DetailTranslationProvider></TooltipProvider>);
  await waitFor(() => expect(useTranslationSettings.getState().settings).not.toBeNull());
  const toggle = screen.getByRole('button', { name: 'translation.action' });
  expect(toggle.textContent).toBe('');
  expect(toggle.closest('.skill-detail-heading')).not.toBeNull();
  expect(toggle.closest('.skill-detail-actions')).toBeNull();
  expect(screen.queryByRole('button', { name: 'translation.more' })).toBeNull();
  expect(screen.getByRole('button', { name: 'Install' }).closest('.skill-detail-actions')).not.toBeNull();
  await user.tab();
  expect(document.activeElement).toBe(toggle);
  expect((await screen.findByRole('tooltip')).textContent).toContain('translation.show');
  await user.keyboard('{Enter}');
  expect(toggle.getAttribute('aria-pressed')).toBe('true');
});

it('配置失败时在内容反馈中提供设置入口', async () => {
  api.translate.mockImplementation(async (units: TranslationUnit[]) => units.map(unit => ({ id: unit.id, status: 'failed', code: 'authentication' })));
  render(<MemoryRouter><TooltipProvider><DetailTranslationProvider documentKey="auth-error" context="Example">
    <TranslationActions /><TranslationStatus /><TranslatedBlock as="p">Read the guide</TranslatedBlock>
  </DetailTranslationProvider></TooltipProvider></MemoryRouter>);
  await waitFor(() => expect(useTranslationSettings.getState().settings).not.toBeNull());
  fireEvent.click(screen.getByRole('button', { name: 'translation.action' }));
  expect(await screen.findByRole('button', { name: 'translation.settings' })).toBeTruthy();
  expect(screen.queryByRole('button', { name: 'translation.more' })).toBeNull();
});
it('开启翻译后 text 围栏仍保留图表原文，不提交翻译请求', async () => {
  render(<TooltipProvider><DetailTranslationProvider documentKey="diagram" context="Example">
    <TranslationActions /><DetailBody loading={false} content={'```text\n┌──────┐\n│ Read │\n└──────┘\n```'} />
  </DetailTranslationProvider></TooltipProvider>);
  await waitFor(() => expect(useTranslationSettings.getState().settings).not.toBeNull());
  fireEvent.click(screen.getByRole('button', { name: 'translation.action' }));
  expect(document.querySelector('pre')?.textContent).toBe('┌──────┐\n│ Read │\n└──────┘\n');
  expect(document.querySelector('[data-translation-output]')).toBeNull();
  expect(api.translate).not.toHaveBeenCalled();
});
it('同一个翻译开关显示和隐藏逐段译文，原文始终存在', async () => {
  api.translate.mockImplementation(async (units: TranslationUnit[]) => units.map(unit => ({ id: unit.id, status: 'translated', text: '阅读指南' })));
  mount();
  await waitFor(() => expect(useTranslationSettings.getState().settings).not.toBeNull());
  const toggle = screen.getByRole('button', { name: 'translation.action' });
  fireEvent.click(toggle);
  expect(await screen.findByText('阅读指南')).toBeTruthy();
  expect(screen.getByText('Read the guide')).toBeTruthy();
  expect(toggle.getAttribute('aria-pressed')).toBe('true');
  fireEvent.click(toggle);
  expect(screen.queryByText('阅读指南')).toBeNull();
  expect(toggle.getAttribute('aria-pressed')).toBe('false');
});

it('从设置返回后保留旧译文，切换服务不会自动产生新请求', async () => {
  api.translate.mockImplementation(async (units: TranslationUnit[]) => units.map(unit => ({ id: unit.id, status: 'translated', text: '旧译文' })));
  const first = mount();
  await waitFor(() => expect(useTranslationSettings.getState().settings).not.toBeNull());
  fireEvent.click(screen.getByRole('button', { name: 'translation.action' }));
  await screen.findByText('旧译文');
  fireEvent.click(screen.getByRole('link', { name: 'settings navigation' }));
  first.unmount();
  api.settings.revision = 'two'; api.settings.serviceRevision = 'two'; api.settings.service = { kind: 'google' };
  api.translate.mockImplementation(async (units: TranslationUnit[]) => units.map(unit => ({ id: unit.id, status: 'translated', text: '新译文' })));
  mount();
  await screen.findByText('translation.settingsChanged');
  expect(screen.getByText('旧译文')).toBeTruthy();
  expect(api.translate).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole('button', { name: 'translation.retranslate' }));
  await screen.findByText('新译文');
  expect(api.translate).toHaveBeenCalledTimes(2);
});
it('关闭后拒绝晚到的成功结果', async () => {
  let resolve!: (values: TranslationResult[]) => void;
  let sent: TranslationUnit[] = [];
  api.translate.mockImplementation((units: TranslationUnit[]) => { sent = units; return new Promise(done => { resolve = done; }); });
  mount();
  await waitFor(() => expect(useTranslationSettings.getState().settings).not.toBeNull());
  const toggle = screen.getByRole('button', { name: 'translation.action' });
  fireEvent.click(toggle);
  await waitFor(() => expect(api.translate).toHaveBeenCalled());
  fireEvent.click(toggle);
  resolve(sent.map(unit => ({ id: unit.id, status: 'translated', text: '旧译文' })));
  await waitFor(() => expect(api.cancel).toHaveBeenCalled());
  expect(screen.queryByText('旧译文')).toBeNull();
});

it('译名单独位于原名下方，不进入原名的截断容器或操作组', async () => {
  const translatedName = '用于规划大型项目模块边界与依赖关系的代码库设计指南';
  api.translate.mockImplementation(async (units: TranslationUnit[]) => units.map(unit => ({ id: unit.id, status: 'translated', text: translatedName })));
  render(<TooltipProvider><DetailTranslationProvider documentKey="translated-header" context="Example">
    <DetailHeader name="codebase-design"><button>Update</button></DetailHeader>
  </DetailTranslationProvider></TooltipProvider>);
  await waitFor(() => expect(useTranslationSettings.getState().settings).not.toBeNull());
  const original = screen.getByRole('heading', { name: 'codebase-design' });
  const toggle = screen.getByRole('button', { name: 'translation.action' });
  fireEvent.click(toggle);
  const translated = await screen.findByText(translatedName);
  expect(original.contains(translated)).toBe(false);
  expect(original.textContent).toBe('codebase-design');
  expect(translated.closest('.truncate')).toBeNull();
  expect(translated.closest('.skill-detail-actions')).toBeNull();
  expect(translated.parentElement).toBe(original.parentElement);
  fireEvent.click(toggle);
  expect(screen.queryByText(translatedName)).toBeNull();
  expect(screen.getByRole('heading', { name: 'codebase-design' })).toBe(original);
});
