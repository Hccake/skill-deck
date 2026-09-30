/* @vitest-environment jsdom */
import '@/test-utils';
import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { toast } from 'sonner';
import { TooltipProvider } from '@/components/ui/tooltip';
import { DetailBody } from '../DetailPrimitives';

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock('sonner', () => ({ toast: { error: vi.fn() } }));

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function documentView(content: string) {
  return <TooltipProvider><DetailBody content={content} loading={false} /></TooltipProvider>;
}

describe('detail Markdown', () => {
  it.each([
    ['language fence', '```python\n  print("<tag> & value")\n\treturn 1  \n\n```', '  print("<tag> & value")\n\treturn 1  \n\n'],
    ['plain fence', '```\nC:\\Review\\skills\n```', 'C:\\Review\\skills\n'],
    ['indented block', '    command --flag\n    next-step\n', 'command --flag\nnext-step\n'],
  ])('copies the parsed %s without its fence or language and keeps focus', async (_name, markdown, expected) => {
    const user = userEvent.setup();
    const copy = vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValue(undefined);
    render(documentView(markdown));
    await user.tab();
    const button = screen.getByRole('button', { name: 'skills.detail.copyCode' });
    expect(document.activeElement).toBe(button);
    await user.keyboard('{Enter}');
    await waitFor(() => expect(copy).toHaveBeenCalledWith(expected));
    expect(screen.getByRole('status').textContent).toBe('skills.detail.codeCopied');
    expect(document.activeElement).toBe(button);
  });

  it('reports a failed copy and lets the same command retry without changing the document', async () => {
    const user = userEvent.setup();
    const copy = vi.spyOn(navigator.clipboard, 'writeText').mockRejectedValueOnce(new Error('denied')).mockResolvedValue(undefined);
    render(documentView('Text with `inline code`.\n\n```sh\necho "done"\n```'));
    const button = screen.getByRole('button', { name: 'skills.detail.copyCode' });
    expect(screen.getAllByRole('button')).toHaveLength(1);
    await user.click(button);
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith('skills.detail.copyCodeFailed'));
    expect(screen.getByRole('status').textContent).toBe('');
    await user.click(button);
    await waitFor(() => expect(screen.getByRole('status').textContent).toBe('skills.detail.codeCopied'));
    expect(copy).toHaveBeenLastCalledWith('echo "done"\n');
    expect(screen.getByText('inline code')).toBeTruthy();
  });

  it('makes only overflowing code and tables keyboard scroll targets as the pane changes width', () => {
    const measure: (() => void)[] = [];
    vi.stubGlobal('ResizeObserver', class {
      constructor(callback: () => void) { measure.push(callback); }
      observe() {}
      disconnect() {}
    });
    vi.spyOn(HTMLElement.prototype, 'scrollWidth', 'get').mockReturnValue(600);
    const width = vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(300);
    render(documentView('```\nlong command\n```\n\n| Field | Value |\n| --- | ---: |\n| retries | 3 |'));
    const code = screen.getByRole('region', { name: 'skills.detail.codeBlock' });
    const table = screen.getByRole('region', { name: 'skills.detail.documentTable' });
    expect(code.tabIndex).toBe(0);
    expect(table.tabIndex).toBe(0);
    expect(screen.getByRole('columnheader', { name: 'Value' }).style.textAlign).toBe('right');
    width.mockReturnValue(700);
    act(() => measure.forEach(callback => callback()));
    expect(code.tabIndex).toBe(-1);
    expect(table.tabIndex).toBe(-1);
    expect(screen.getByRole('cell', { name: '3' })).toBeTruthy();
  });
});
