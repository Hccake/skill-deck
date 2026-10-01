/* @vitest-environment jsdom */
import '@/test-utils';
import { act, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { DiscoverRichText } from '../DiscoverRichText';

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe('Discover HTML reading', () => {
  afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });

  it('makes only overflowing code and tables keyboard scrollable and preserves their text', () => {
    let width = 240;
    let measure = () => {};
    vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockImplementation(() => width);
    vi.spyOn(HTMLElement.prototype, 'scrollWidth', 'get').mockImplementation(function (this: HTMLElement) {
      return this.tagName === 'PRE' || this.classList.contains('discover-table-scroll') ? 500 : width;
    });
    vi.stubGlobal('ResizeObserver', class {
      constructor(callback: () => void) { measure = callback; }
      observe() {} disconnect() {}
    });
    const { container, rerender } = render(<DiscoverRichText html={'<pre><code>  first\nsecond\n</code></pre><table><tbody><tr><td>value</td></tr></tbody></table>'} />);
    const code = screen.getByRole('region', { name: 'skills.detail.codeBlock' });
    const table = screen.getByRole('region', { name: 'skills.detail.documentTable' });
    expect(code.tabIndex).toBe(0);
    expect(table.tabIndex).toBe(0);
    expect(code.textContent).toBe('  first\nsecond\n');
    expect(screen.getByRole('table').parentElement).toBe(table);
    act(() => { width = 640; measure(); });
    expect(code.tabIndex).toBe(-1);
    expect(table.tabIndex).toBe(-1);
    expect(screen.queryByRole('region')).toBeNull();
    rerender(<DiscoverRichText html="<p>New content</p>" />);
    expect(screen.getByText('New content')).toBeTruthy();
    expect(container.querySelector('table')).toBeNull();
  });
});
