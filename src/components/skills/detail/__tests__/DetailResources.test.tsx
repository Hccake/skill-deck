/* @vitest-environment jsdom */
import '@/test-utils';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { openUrl } from '@tauri-apps/plugin-opener';
import { toast } from 'sonner';
import { TooltipProvider } from '@/components/ui/tooltip';
import { DetailBody } from '../DetailPrimitives';

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn(async () => {}) }));
vi.mock('sonner', () => ({ toast: { error: vi.fn() } }));
beforeEach(() => vi.clearAllMocks());
afterEach(() => vi.restoreAllMocks());

const documentView = (content: string) => <TooltipProvider><div data-radix-scroll-area-viewport=""><DetailBody content={content} loading={false} /></div></TooltipProvider>;

describe('detail document links', () => {
  it('opens the exact external address from keyboard, modified click and middle click without navigating the app', async () => {
    const user = userEvent.setup();
    const target = 'https://example.com/guide?q=one%20two#section';
    const route = location.href;
    render(documentView(`[Guide](${target})`));
    await user.tab();
    await user.keyboard('{Enter}');
    fireEvent.click(screen.getByRole('link', { name: 'Guide' }), { ctrlKey: true });
    fireEvent(screen.getByRole('link', { name: 'Guide' }), new MouseEvent('auxclick', { button: 1, bubbles: true, cancelable: true }));
    await waitFor(() => expect(openUrl).toHaveBeenCalledTimes(3));
    expect(openUrl).toHaveBeenLastCalledWith(target);
    expect(location.href).toBe(route);
  });

  it('keeps a failed external link available for retry', async () => {
    vi.mocked(openUrl).mockRejectedValueOnce(new Error('opener unavailable'));
    const user = userEvent.setup();
    render(documentView('[Guide](https://example.com/guide)'));
    const link = screen.getByRole('link', { name: 'Guide' });
    await user.click(link);
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith('skills.detail.linkOpenFailed'));
    await user.click(link);
    expect(openUrl).toHaveBeenCalledTimes(2);
  });

  it.each(['references/config.md?mode=full#setup', 'references/使用说明.md#安装', 'file:///C:/Review/config.md', 'javascript:alert%281%29', 'mailto:team@example.com'])('copies an unsupported target unchanged and never renders it as navigation: %s', async target => {
    const user = userEvent.setup();
    const copy = vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValue(undefined);
    render(documentView(`[Reference](${target})`));
    expect(screen.queryByRole('link')).toBeNull();
    await user.click(screen.getByRole('button', { name: 'skills.detail.copyReferenceAddress' }));
    expect(copy).toHaveBeenCalledWith(target);
    expect(openUrl).not.toHaveBeenCalled();
    expect(screen.getByText('Reference')).toBeTruthy();
  });

  it('uses stable scoped anchors for repeated formatted headings and only scrolls the active document', async () => {
    const user = userEvent.setup();
    const content = '[Second](#%E9%87%8D%E5%A4%8D-%E6%A0%87%E9%A2%98-1)\n\n# Intro\n\n## 重复 `标题`\n\n## 重复 `标题`';
    const view = render(<>{documentView(content)}{documentView(content)}</>);
    const viewports = [...view.container.querySelectorAll<HTMLElement>('[data-radix-scroll-area-viewport]')];
    const scrolls = viewports.map(viewport => {
      viewport.scrollTop = 100;
      viewport.scrollTo = vi.fn();
      vi.spyOn(viewport, 'getBoundingClientRect').mockReturnValue({ top: 100 } as DOMRect);
      return viewport.scrollTo;
    });
    const headings = screen.getAllByRole('heading', { name: '重复 标题' });
    const ids = headings.map(heading => heading.id);
    expect(new Set(ids).size).toBe(4);
    vi.spyOn(headings[1], 'getBoundingClientRect').mockReturnValue({ top: 450 } as DOMRect);
    const route = location.href;
    await user.click(within(viewports[0]).getByRole('link', { name: 'Second' }));
    expect(document.activeElement).toBe(headings[1]);
    expect(scrolls[0]).toHaveBeenCalledWith({ top: 442, behavior: 'instant' });
    expect(scrolls[1]).not.toHaveBeenCalled();
    expect(location.href).toBe(route);
    view.rerender(<>{documentView(content)}{documentView(content)}</>);
    expect(screen.getAllByRole('heading', { name: '重复 标题' }).map(heading => heading.id)).toEqual(ids);
  });

  it('returns to the document start before its first heading', async () => {
    const user = userEvent.setup();
    const view = render(documentView('Introductory paragraph. [Start](#)\n\n# Later heading'));
    const viewport = view.container.querySelector<HTMLElement>('[data-radix-scroll-area-viewport]')!;
    viewport.scrollTo = vi.fn();
    await user.click(screen.getByRole('link', { name: 'Start' }));
    expect(document.activeElement).toBe(view.container.querySelector('.skill-markdown'));
    expect(viewport.scrollTo).toHaveBeenCalledWith({ top: 0, behavior: 'instant' });
  });

  it('preserves reference-style destinations and GFM footnote navigation', async () => {
    const user = userEvent.setup();
    const copy = vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValue(undefined);
    const view = render(documentView('[Guide][notes]\n\nA note[^first].\n\n[notes]: references/说明.md#开始\n\n[^first]: Footnote text.'));
    const viewport = view.container.querySelector<HTMLElement>('[data-radix-scroll-area-viewport]')!;
    viewport.scrollTo = vi.fn();
    await user.click(screen.getByRole('button', { name: 'skills.detail.copyReferenceAddress' }));
    expect(copy).toHaveBeenCalledWith('references/说明.md#开始');
    const reference = screen.getByRole('link', { name: '1' });
    expect(document.getElementById(reference.getAttribute('aria-describedby')!)?.textContent).toBe('Footnotes');
    await user.click(reference);
    expect(document.activeElement?.textContent).toContain('Footnote text.');
    expect(viewport.scrollTo).toHaveBeenCalled();
  });

  it.each(['#missing', '#%not-encoded'])('preserves position and reports a missing or invalid chapter: %s', async fragment => {
    const user = userEvent.setup();
    const view = render(documentView(`[Jump](${fragment})\n\n# Existing`));
    const viewport = view.container.querySelector<HTMLElement>('[data-radix-scroll-area-viewport]')!;
    viewport.scrollTo = vi.fn();
    await user.click(screen.getByRole('link', { name: 'Jump' }));
    expect(toast.error).toHaveBeenCalledWith('skills.detail.chapterMissing');
    expect(viewport.scrollTo).not.toHaveBeenCalled();
  });
});

describe('detail document images', () => {
  it('retries only the failed image with the exact URL and transfers focus when retry disappears', async () => {
    const user = userEvent.setup();
    const source = 'https://example.com/diagram.png?signature=abc%2B123';
    render(documentView(`![Diagram](${source})\n\n![Other](https://example.com/other.png)`));
    const original = screen.getByRole('img', { name: 'Diagram' });
    const other = screen.getByRole('img', { name: 'Other' });
    fireEvent.error(original);
    const retry = screen.getByRole('button', { name: 'skills.detail.retryImage' });
    await user.click(retry);
    expect(retry.getAttribute('aria-disabled')).toBe('true');
    const retryImage = screen.getByAltText('Diagram');
    expect(retryImage).not.toBe(original);
    expect(retryImage.getAttribute('src')).toBe(source);
    expect(screen.getByRole('img', { name: 'Other' })).toBe(other);
    fireEvent.load(retryImage);
    await waitFor(() => expect(document.activeElement).toBe(retryImage));
    expect(screen.queryByRole('button', { name: 'skills.detail.retryImage' })).toBeNull();
  });

  it('keeps image retry outside its link and preserves both navigation and formatted link text', async () => {
    const user = userEvent.setup();
    const view = render(documentView('[**Guide** ![Diagram](https://example.com/image.png) details](https://example.com/guide)'));
    fireEvent.error(screen.getByRole('img', { name: 'Diagram' }));
    const retry = screen.getByRole('button', { name: 'skills.detail.retryImage' });
    expect(retry.closest('a')).toBeNull();
    expect(view.container.querySelector('a button, a a')).toBeNull();
    expect(view.container.querySelector('strong')?.textContent).toBe('Guide');
    await user.click(screen.getByRole('link', { name: 'Guide' }));
    expect(openUrl).toHaveBeenCalledWith('https://example.com/guide');
    await user.click(retry);
    expect(openUrl).toHaveBeenCalledTimes(1);
    fireEvent.load(screen.getByAltText('Diagram'));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('link', { name: 'Diagram' })));
  });

  it.each(['images/流程图.png', 'http://example.com/image.png', 'data:image/png;base64,AAAA'])('copies an unsupported image address without attempting to load it: %s', async target => {
    const user = userEvent.setup();
    const copy = vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValue(undefined);
    const view = render(documentView(`![Diagram](${target})`));
    expect(view.container.querySelector('img')).toBeNull();
    expect(screen.queryByRole('button', { name: 'skills.detail.retryImage' })).toBeNull();
    await user.click(screen.getByRole('button', { name: 'skills.detail.copyImageAddress' }));
    expect(copy).toHaveBeenCalledWith(target);
  });

  it('resets failure state for another image and ignores events from the old image', () => {
    const view = render(documentView('![Old](https://example.com/old.png)'));
    const old = screen.getByRole('img', { name: 'Old' });
    fireEvent.error(old);
    view.rerender(documentView('![New](https://example.com/new.png)'));
    const next = screen.getByRole('img', { name: 'New' });
    fireEvent.error(old);
    expect(screen.getByRole('img', { name: 'New' })).toBe(next);
    expect(screen.queryByRole('button', { name: 'skills.detail.retryImage' })).toBeNull();
  });
});
