/* @vitest-environment jsdom */
import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { CompactSkillItem } from '../CompactSkillItem';
import { LibraryCompactItem } from '@/components/library/LibraryCompactItem';
import type { InstalledSkill, LibrarySkillSummary } from '@/bindings';

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe('compact Skill sources', () => {
  it('uses the local-source label for installed Skills and keeps the original selection target', () => {
    const skill = { name: 'toolkit', scope: 'global', source: '/source/toolkit', updateReason: 'local-source' } as InstalledSkill;
    const select = vi.fn();
    render(<TooltipProvider><CompactSkillItem skill={skill} isSelected={false} onClick={select} /></TooltipProvider>);
    expect(screen.getByText('skills.updateStatusLabel.localSource')).toBeTruthy();
    expect(screen.queryByText('/source/toolkit')).toBeNull();
    fireEvent.click(screen.getByRole('button'));
    expect(select).toHaveBeenCalledWith(skill);
  });

  it('uses the same local-source label for Library members', () => {
    const skill = { name: 'toolkit', source: '/source/toolkit', sourceType: 'local' } as LibrarySkillSummary;
    const select = vi.fn();
    render(<TooltipProvider><LibraryCompactItem skill={skill} isSelected onClick={select} /></TooltipProvider>);
    expect(screen.getByText('skills.updateStatusLabel.localSource')).toBeTruthy();
    expect(screen.queryByText('/source/toolkit')).toBeNull();
    fireEvent.click(screen.getByRole('button'));
    expect(select).toHaveBeenCalledWith('toolkit');
  });

  it('keeps missing source information distinct from a local Skill', () => {
    const skill = { name: 'toolkit', source: '', sourceType: 'git' } as LibrarySkillSummary;
    render(<TooltipProvider><LibraryCompactItem skill={skill} isSelected={false} onClick={vi.fn()} /></TooltipProvider>);
    expect(screen.getByText('skills.card.sourceIncomplete')).toBeTruthy();
    expect(screen.queryByText('skills.updateStatusLabel.localSource')).toBeNull();
  });
});
