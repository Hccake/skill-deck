/* @vitest-environment jsdom */
import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { SourceFilterCombobox } from '../SourceFilterCombobox';

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
const option = { key: 'repo', kind: 'git' as const, label: 'acme/tools', detail: 'github.com', count: 2 };

describe('SourceFilterCombobox', () => {
  it('searches hostnames and selects the exact source with the keyboard', () => {
    const onChange = vi.fn();
    render(<TooltipProvider><SourceFilterCombobox options={[option]} selected={null} onChange={onChange} totalCount={2} /></TooltipProvider>);
    fireEvent.click(screen.getByRole('button', { name: 'skills.filter.sourceLabel' }));
    const input = screen.getByRole('combobox');
    fireEvent.change(input, { target: { value: 'github' } });
    fireEvent.keyDown(input, { key: 'ArrowDown' });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onChange).toHaveBeenCalledWith(option);
  });

  it('can restore all sources and Escape does not close the parent workspace', () => {
    const onChange = vi.fn();
    const parent = vi.fn();
    render(<TooltipProvider><div onKeyDown={parent}><SourceFilterCombobox options={[option]} selected={option} onChange={onChange} totalCount={2} /></div></TooltipProvider>);
    fireEvent.click(screen.getByRole('button', { name: 'skills.filter.sourceLabel' }));
    fireEvent.keyDown(screen.getByRole('combobox'), { key: 'Escape' });
    expect(parent).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'skills.filter.sourceLabel' }));
    fireEvent.click(screen.getByRole('option', { name: 'skills.filter.allSources (2)' }));
    expect(onChange).toHaveBeenCalledWith(null);
  });
});
