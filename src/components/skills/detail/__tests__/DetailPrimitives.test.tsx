/* @vitest-environment jsdom */
import '@/test-utils';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { openUrl } from '@tauri-apps/plugin-opener';
import { TooltipProvider } from '@/components/ui/tooltip';
import { DetailBody, DetailDescription, DetailHeader, DetailSourceLink } from '../DetailPrimitives';

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn(async () => {}) }));

afterEach(() => vi.restoreAllMocks());

describe('DetailHeader', () => {
  it('keeps a failed content refresh visible in the fixed header', async () => {
    render(<TooltipProvider><DetailHeader name="review" contentState="stale"><button>Close</button></DetailHeader></TooltipProvider>);
    const status = screen.getByRole('img', { name: 'skills.detail.refreshFailed' });
    act(() => status.focus());
    expect((await screen.findByRole('tooltip')).textContent).toBe('skills.detail.refreshFailed');
  });
  it('reveals a truncated name on keyboard focus without turning it into a command', async () => {
    vi.spyOn(HTMLElement.prototype, 'scrollWidth', 'get').mockReturnValue(400);
    vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(160);
    render(<TooltipProvider><DetailHeader name="cross-platform-testing-and-validation"><button>Close</button></DetailHeader></TooltipProvider>);
    const name = screen.getByRole('heading', { name: 'cross-platform-testing-and-validation' });
    expect(name.tabIndex).toBe(0);
    act(() => name.focus());
    expect((await screen.findByRole('tooltip')).textContent).toBe(name.textContent);
    expect(screen.queryByRole('button', { name: name.textContent! })).toBeNull();
  });
});

describe('DetailBody', () => {
  it('preserves an authored heading and paragraph even when they match the overview', () => {
    render(<TooltipProvider>
      <DetailHeader name="review"><button>Close</button></DetailHeader>
      <DetailDescription>Review code changes.</DetailDescription>
      <DetailBody content={'# review\n\nReview code changes.\n\n## Workflow\n\nFollow the data flow.'} loading={false} />
    </TooltipProvider>);
    expect(screen.getByRole('heading', { level: 1, name: 'review' })).toBeTruthy();
    expect(screen.getAllByText('Review code changes.')).toHaveLength(2);
    expect(screen.getByRole('heading', { level: 2, name: 'Workflow' })).toBeTruthy();
  });

  it('keeps the same document mounted while reloading and after a refresh failure', () => {
    const retry = vi.fn();
    const view = render(<DetailBody content="Readable document" loading={false} onRetry={retry} />);
    const paragraph = screen.getByText('Readable document');
    view.rerender(<DetailBody content="Readable document" loading onRetry={retry} />);
    expect(screen.getByText('Readable document')).toBe(paragraph);
    expect(screen.getByRole('status').textContent).toContain('skills.detail.refreshing');
    view.rerender(<DetailBody content="Readable document" loading={false} errorMessage="Read failed" onRetry={retry} />);
    expect(screen.getByText('Readable document')).toBe(paragraph);
    expect(screen.getByRole('alert').textContent).toContain('skills.detail.showingPreviousContent');
    fireEvent.click(screen.getByRole('button', { name: 'skills.detail.retry' }));
    expect(retry).toHaveBeenCalledTimes(1);
  });
});

describe('DetailDescription', () => {
  it('lets the user expand and collapse a long description', () => {
    vi.spyOn(HTMLElement.prototype, 'scrollHeight', 'get').mockReturnValue(88);
    render(<TooltipProvider><DetailDescription>Full description</DetailDescription></TooltipProvider>);
    const expand = screen.getByRole('button', { name: 'skills.detail.expandDescription' });
    expect(expand.getAttribute('aria-expanded')).toBe('false');
    expect(document.getElementById(expand.getAttribute('aria-controls')!)?.textContent).toBe('Full description');
    fireEvent.click(expand);
    const collapse = screen.getByRole('button', { name: 'skills.detail.collapseDescription' });
    expect(collapse.getAttribute('aria-expanded')).toBe('true');
    fireEvent.click(collapse);
    expect(screen.getByRole('button', { name: 'skills.detail.expandDescription' }).getAttribute('aria-expanded')).toBe('false');
  });

  it('shows short descriptions without an expansion command', () => {
    render(<TooltipProvider><DetailDescription>Short description</DetailDescription></TooltipProvider>);
    expect(screen.getByText('Short description')).toBeTruthy();
    expect(screen.queryByRole('button')).toBeNull();
  });
});

describe('DetailSourceLink', () => {
  it('reveals the complete source on focus without requiring a diagnostic hint', async () => {
    const source = 'https://skills.example.test/platform/team/very-long-api-and-release-guidelines';
    render(<TooltipProvider><DetailSourceLink label={source} url={source} /></TooltipProvider>);
    const link = screen.getByRole('button', { name: source });
    act(() => link.focus());
    expect((await screen.findByRole('tooltip')).textContent).toContain(source);
    fireEvent.click(link);
    expect(openUrl).toHaveBeenCalledWith(source);
  });

  it('discloses an unopenable source as focusable text with its diagnostic', async () => {
    render(<TooltipProvider><DetailSourceLink label="git@example.test:team/skills.git" url="file:///private/skills" hint="Source information is incomplete" /></TooltipProvider>);
    expect(screen.queryByRole('button')).toBeNull();
    act(() => screen.getByText('git@example.test:team/skills.git').parentElement!.focus());
    const tooltip = await screen.findByRole('tooltip');
    expect(tooltip.textContent).toContain('git@example.test:team/skills.git');
    expect(tooltip.textContent).toContain('Source information is incomplete');
  });
});
