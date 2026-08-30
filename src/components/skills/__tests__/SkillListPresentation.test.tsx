/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { MemoryRouter } from 'react-router-dom';
import { SkillsSection } from '../SkillsSection';
import type { SkillListItem } from '@/stores/skills-utils';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: 'en' },
  }),
}));

vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: vi.fn().mockResolvedValue(undefined),
}));

describe('installed Skill list presentation', () => {
  it('renders ownership, source, attention facts, and associated Agents together', () => {
    const skill = {
      name: 'toolkit',
      pluginName: 'claude-code-tools',
      description: 'Toolkit description',
      path: '/skills/toolkit',
      canonicalPath: '/canonical/toolkit',
      scope: 'global',
      agents: [],
      associatedAgents: ['claude-code'],
      hasUpdate: false,
      canRunUpdate: false,
      canCheckForUpdates: false,
      updateReason: 'upstreamUnavailable',
      updateAttempt: { outcome: 'notCompleted', reason: 'upstreamUnavailable' },
      source: 'owner/repo',
      sourceUrl: 'https://github.com/owner/repo',
      duplicateCopyCount: 1,
    } as SkillListItem;

    render(
      <MemoryRouter>
        <TooltipProvider>
          <SkillsSection
          title="Global"
          skills={[skill]}
          scope="global"
          duplicateLocationSkillNames={new Set(['toolkit'])}
          updatingSkills={new Map()}
          agentDisplayNames={new Map([['claude-code', 'Claude Code']])}
          onSkillClick={vi.fn()}
          onPrepareUpdate={vi.fn(async () => true)}
          onDelete={vi.fn()}
          onAdd={vi.fn()}
          />
        </TooltipProvider>
      </MemoryRouter>
    );

    const title = screen.getByTestId('skill-card-title');
    expect(within(title).getByText('toolkit')).toBeTruthy();
    expect(within(title).getByText('Claude Code Tools')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'owner/repo' })).toBeTruthy();

    const attention = screen.getByTestId('skill-card-attention');
    expect(within(attention).getByText('skills.card.updateCheckIncomplete')).toBeTruthy();
    expect(within(attention).getByText('skills.card.duplicateLocations')).toBeTruthy();
    expect(within(attention).getByText('skills.card.duplicateAgentInstall')).toBeTruthy();
    expect(attention.querySelectorAll('svg')).toHaveLength(1);
    expect(screen.getByText('Claude Code')).toBeTruthy();
  });
});
