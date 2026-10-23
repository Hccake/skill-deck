/* @vitest-environment jsdom */
import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { GitReference, GitReferenceField } from '../GitReference';

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe('Git reference presentation', () => {
  it('does not infer a pin from a hexadecimal legacy name', () => {
    render(<TooltipProvider><GitReference value={'a'.repeat(40)} /></TooltipProvider>);
    expect(screen.getByText('skills.reference.unknown')).toBeTruthy();
    expect(screen.getByText('a'.repeat(40))).toBeTruthy();
    expect(screen.queryByText('skills.reference.commit')).toBeNull();
  });

  it('shortens only confirmed commits and discloses the full value on focus', async () => {
    const sha = 'abcdef0123456789abcdef0123456789abcdef01';
    render(<TooltipProvider delayDuration={0}><GitReference value={sha} kind="commit" /></TooltipProvider>);
    expect(screen.getByText('abcdef01')).toBeTruthy();
    fireEvent.focus(screen.getByText('abcdef01').closest('[tabindex]')!);
    expect((await screen.findByRole('tooltip')).textContent).toContain(sha);
  });

  it('copies the full commit from the detail field', async () => {
    const copy = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: copy } });
    const sha = 'a'.repeat(40);
    render(<TooltipProvider><GitReferenceField value={sha} kind="commit" /></TooltipProvider>);
    fireEvent.click(screen.getByRole('button', { name: 'skills.reference.copy' }));
    expect(copy).toHaveBeenCalledWith(sha);
    expect(screen.getByText('skills.reference.commit')).toBeTruthy();
  });
});
