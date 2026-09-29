/* @vitest-environment jsdom */
import '@/test-utils';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { openUrl } from '@tauri-apps/plugin-opener';
import { TooltipProvider } from '@/components/ui/tooltip';
import { DetailDescription, DetailSourceLink } from '../DetailPrimitives';

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn(async () => {}) }));

afterEach(() => vi.restoreAllMocks());

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
