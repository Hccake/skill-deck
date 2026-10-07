import '@/test-utils';
import { beforeEach, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { TooltipProvider } from '@/components/ui/tooltip';
import { DetailTranslationProvider, TranslationActions } from '../../detail/DetailTranslation';
import { DiscoverRichText } from '../DiscoverRichText';
import { useTranslationSettings, translationPreferences } from '@/stores/translation';
import type { TranslationUnit } from '@/bindings';
import { clearReadingReturn } from '@/lib/translation/navigation';

const requests = vi.hoisted(() => ({ translate: vi.fn(async (_session: string, units: TranslationUnit[]) => units.map(unit => ({ id: unit.id, status: 'translated', text: unit.text.replace('Read', '阅读') }))) }));

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock('@/hooks/useTauriApi', () => ({
  getTranslationSettings: async () => ({ revision: 'one', serviceRevision: 'microsoft', service: { kind: 'microsoft' }, profiles: [], autoTranslate: false, noticeAccepted: true }),
  beginTranslation: async () => 'session', cancelTranslation: async () => {},
  translateUnits: requests.translate,
}));
beforeEach(() => {
  useTranslationSettings.setState({ settings: null, error: null });
  translationPreferences.clear(); clearReadingReturn(); requests.translate.mockClear();
});
it('发现页的受控 HTML 按段配对并保留代码和链接', async () => {
  useTranslationSettings.setState({ settings: null, error: null }); translationPreferences.clear();
  const view = () => <TooltipProvider><DetailTranslationProvider documentKey="discover-one" context="Example"><TranslationActions />
    <DiscoverRichText html='<p>Read <code>npm install</code> and <a href="https://example.com">the guide</a></p>' />
  </DetailTranslationProvider></TooltipProvider>;
  const rendered = render(view());
  await waitFor(() => expect(useTranslationSettings.getState().settings).not.toBeNull());
  fireEvent.click(screen.getByRole('button', { name: 'translation.action' }));
  await waitFor(() => expect(document.querySelector('[data-translation-output]:not([hidden])')?.textContent).toContain('阅读'));
  const output = document.querySelector('[data-translation-output]:not([hidden])')!;
  expect(output.querySelector('code')?.textContent).toBe('npm install');
  expect(output.querySelector('a')?.getAttribute('href')).toBe('https://example.com');
  rendered.rerender(view());
  expect(document.querySelector('[data-translation-output]:not([hidden])')?.textContent).toContain('阅读');
});

it('仅有 pre 的内容不会被兜底成整个容器的译文', async () => {
  render(<TooltipProvider><DetailTranslationProvider documentKey="pre-only" context="Example"><TranslationActions />
    <DiscoverRichText html='<pre>Read the diagram unchanged</pre>' />
  </DetailTranslationProvider></TooltipProvider>);
  await waitFor(() => expect(useTranslationSettings.getState().settings).not.toBeNull());
  fireEvent.click(screen.getByRole('button', { name: 'translation.action' }));
  expect(document.querySelector('[data-translation-slot]')).toBeNull();
  expect(document.querySelector('pre')?.textContent).toBe('Read the diagram unchanged');
  expect(requests.translate).not.toHaveBeenCalled();
});
